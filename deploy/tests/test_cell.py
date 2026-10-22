"""Invariants of the workspace chart and the Compose file (E21-36), on what they render.

Each property of the story's acceptance criteria is one function returning the problems it finds,
asserted empty on the real rendering and shown non-empty on a seeded bug in
`test_each_seeded_bug_is_caught`. The chart emits one JSON document per manifest, so no YAML parser
is needed. Without helm these tests skip, unless MANDATE_CELL_HELM_REQUIRED is set, as CI's cell job
sets it (`cargo xtask ci cell`).
"""

import copy
import functools
import json
import os
import re
import shutil
import subprocess
from collections.abc import Iterator
from pathlib import Path

import celpy
import pytest
from celpy import celtypes

CELL = Path(__file__).resolve().parents[1] / "cell"
CHART = CELL / "charts" / "workspace"
JOURNAL_CHART = CELL / "charts" / "journal-db"
# The journal-db chart as the local cell renders it, with the Kubernetes API's address a rendering
# without a cluster must be given (E21-41).
JOURNAL_ARGS = (
    "-f",
    str(CELL / "values" / "k3d-journal-db.yaml"),
    "--set-json",
    'kubeApi={"cidrs":["10.43.0.1/32"],"ports":[443]}',
)
PROCESSES = json.loads((CHART / "processes.json").read_text())["processes"]
COMPOSE = json.loads((CELL / "compose" / "compose.json").read_text())
ROLES_SQL = (JOURNAL_CHART / "files" / "roles.sql").read_text()
VALUES_FILES = ("k3d.yaml", "k3s.yaml", "managed.yaml")
PLACEHOLDER = "registry.example/mandate/{}@sha256:" + "0" * 64
IN_CLUSTER = {"postgres", "vault", "model-gateway"}
# Infrastructure §3.1's network column, written out here from the design, not read from
# processes.json, so a list that drifts from the design fails (AGENTS.md, independent oracles).
# Each destination is (target, protocol, port); `dns` is added for any process with a destination.
SECTION_3_1 = {
    "migrate": {("postgres", "TCP", 5432)},
    "api": {("postgres", "TCP", 5432), ("identity-provider", "TCP", 443)},
    "control": {("postgres", "TCP", 5432), ("vault", "TCP", 8200), ("identity-provider", "TCP", 443)},
    "journal-verify": {("postgres", "TCP", 5432)},
    "connect-check": {("postgres", "TCP", 5432)},
    "runtime": {("postgres", "TCP", 5432), ("model-gateway", "TCP", 8080), ("artifact-store", "TCP", 443)},
    "executor": {("postgres", "TCP", 5432), ("vault", "TCP", 8200), ("broker", "TCP", 443)},
    "token-exchange": {("vault", "TCP", 8200), ("token-host", "TCP", 443)},
    "scheduler": {("postgres", "TCP", 5432), ("time", "UDP", 123)},
    "model-gateway": {
        ("postgres", "TCP", 5432),
        ("vault", "TCP", 8200),
        ("model-gateway", "TCP", 8080),
        ("model-providers", "TCP", 443),
    },
    "exporter": {("postgres", "TCP", 5432), ("object-storage", "TCP", 443), ("timestamping", "TCP", 443)},
    "data": {("market-data", "TCP", 443)},
    "deploy-manager": {("postgres", "TCP", 5432), ("kube-api", "TCP", 6443)},
}
# The API server's endpoint the tests give the deployment manager (DEC-1172), as `cargo xtask cell
# up` gives it the cell's own; the manager is the one process that may reach it.
KUBE_API = "192.0.2.200/32"
MANAGER = "deploy-manager"
TEMPLATES = "deploy-manager-templates"
HOOK = "pre-install,pre-upgrade"
# Where the interim runtime image carries the recorded research bundle (DEC-2246).
RESEARCH = "/usr/local/share/mandate/research"

if shutil.which("helm") is None:
    if os.environ.get("MANDATE_CELL_HELM_REQUIRED"):
        raise RuntimeError("MANDATE_CELL_HELM_REQUIRED is set but helm is not installed")
    pytest.skip("helm is not installed (deploy/cell/README.md)", allow_module_level=True)


def helm(chart: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["helm", "template", "cell", str(chart), "--namespace", "ws-ws1", *args],
        capture_output=True,
        text=True,
        check=False,
    )


def documents(text: str) -> list[dict]:
    lines = (line.strip() for line in text.splitlines())
    return [json.loads(line) for line in lines if line and line != "---" and not line.startswith("#")]


def render(*args: str, chart: Path = CHART) -> list[dict]:
    done = helm(chart, *args)
    assert done.returncode == 0, done.stderr
    return documents(done.stdout)


def every_image() -> list[str]:
    sets = [
        f"images.{p['image']}={PLACEHOLDER.format(p['image'])}" for p in PROCESSES if p["image"] != "postgres"
    ]
    return [arg for s in sets for arg in ("--set", s)]


def render_all(*extra: str) -> list[dict]:
    """Every process type rendered, with a one-off run and a verification of two streams."""
    return render(
        "-f",
        str(CELL / "values" / "k3d.yaml"),
        *every_image(),
        "--set",
        "run.id=t1",
        "--set-json",
        'run.args=["workspace","open","--workspace","ws9"]',
        "--set",
        "verify.id=t1",
        "--set-json",
        'verify.streams=["ctl:ws9","ctl:ws8"]',
        "--set-json",
        f'egress.kube-api=["{KUBE_API}"]',
        *extra,
    )


def process(name: str) -> dict:
    return next(p for p in PROCESSES if p["name"] == name)


def managed_pod(name: str, pod: dict) -> dict:
    """A pod template of the manager's ConfigMap as a workload, so every pod check reads it too."""
    return {
        "kind": "Pod",
        "metadata": {"name": name, "labels": pod["metadata"]["labels"]},
        "spec": {"template": pod},
    }


def managed_pods(docs: list[dict]) -> list[dict]:
    """The pods the deployment manager starts: its ConfigMap's `<process>.json` entries (DEC-1170)."""
    maps = [d for d in docs if d["kind"] == "ConfigMap" and d["metadata"]["name"] == TEMPLATES]
    return [
        managed_pod(key.removesuffix(".json"), json.loads(text))
        for m in maps
        for key, text in sorted((m.get("data") or {}).items())
    ]


def workloads(docs: list[dict]) -> list[dict]:
    return [d for d in docs if d["kind"] in ("Deployment", "Job")] + managed_pods(docs)


def is_manager(workload: dict) -> bool:
    return workload["kind"] == "Deployment" and workload["metadata"]["name"] == MANAGER


def pod_spec(workload: dict) -> dict:
    return workload["spec"]["template"]["spec"]


def containers(workload: dict) -> list[dict]:
    spec = pod_spec(workload)
    return [*spec.get("initContainers", []), *spec["containers"]]


def owner(workload: dict) -> dict:
    return process(workload["metadata"]["labels"]["mandate.dev/process"])


def policies(docs: list[dict]) -> dict[str, dict]:
    return {d["metadata"]["name"]: d for d in docs if d["kind"] == "NetworkPolicy"}


def one_workload_per_process(docs: list[dict], run_process: str = "control") -> list[str]:
    """Every process type renders exactly one Deployment (a service) or Job, and nothing else; once
    the deployment manager renders, the processes it starts (`started_by`) are only its pod
    templates, which it alone starts (DEC-1170), and a `one_shot` one (the token exchange) is never
    a Deployment, with or without it (DEC-1175). Of the `run` processes, only the one a run names
    renders, `control` unless it names another (DEC-2035)."""
    kinds = {"service": "Deployment", "migration": "Job", "run": "Job", "verify": "Job"}
    manager = any(is_manager(w) for w in workloads(docs))
    problems = []
    for p in PROCESSES:
        found = [
            w for w in workloads(docs) if w["metadata"]["labels"].get("mandate.dev/process") == p["name"]
        ]
        want = "Pod" if manager and p.get("started_by") == MANAGER else kinds[p["kind"]]
        if p.get("one_shot") and not manager:
            want = None
        if p["kind"] == "run" and p["name"] != run_process:
            want = None
        if [w["kind"] for w in found] != ([want] if want else []):
            problems.append(f"{p['name']}: {[w['kind'] for w in found]}")
    return problems


def mounts_the_store(workload: dict) -> bool:
    return any(
        v.get("persistentVolumeClaim", {}).get("claimName") == "artifacts" for v in pod_spec(workload).get("volumes", [])
    )


def colocated(docs: list[dict]) -> list[str]:
    """A workspace's executors, runtimes and data service share a node by required pod affinity (§3.2),
    and so does every pod that mounts the artifact store, a ReadWriteOnce claim, as one group
    (DEC-2273): the store's mounts are read from each pod's volumes, not from the chart's helper."""
    problems = []
    for w in workloads(docs):
        affinity = pod_spec(w).get("affinity", {}).get("podAffinity", {})
        terms = affinity.get("requiredDuringSchedulingIgnoredDuringExecution", [])
        want = owner(w)["colocate"] or mounts_the_store(w)
        good = [
            t
            for t in terms
            if t["topologyKey"] == "kubernetes.io/hostname"
            and t["labelSelector"]["matchLabels"]
            == {"mandate.dev/workspace": "ws1", "mandate.dev/colocate": "true"}
        ]
        labelled = w["spec"]["template"]["metadata"]["labels"].get("mandate.dev/colocate") == "true"
        if want != bool(good) or want != labelled:
            problems.append(
                f"{w['metadata']['name']}: colocate {want}, affinity {bool(good)}, label {labelled}"
            )
    for name in ("runtime", "executor", "data"):
        if not process(name)["colocate"]:
            problems.append(f"{name} is not colocated")
    mounting = {w["metadata"]["labels"]["mandate.dev/process"] for w in workloads(docs) if mounts_the_store(w)}
    if mounting and not {"api", "model-gateway"} <= mounting:
        problems.append(f"the store's mounts are {sorted(mounting)}, without the API server or the model gateway")
    return problems


def guaranteed(docs: list[dict]) -> list[str]:
    """Every container has CPU and memory limits; the executor's and the scheduler's requests equal
    them (§3.3)."""
    problems = []
    for w in workloads(docs):
        for c in containers(w):
            limits, requests = c["resources"].get("limits", {}), c["resources"].get("requests", {})
            if set(limits) != {"cpu", "memory"}:
                problems.append(f"{w['metadata']['name']}/{c['name']}: limits {limits}")
            if owner(w)["name"] in ("executor", "scheduler") and requests != limits:
                problems.append(
                    f"{w['metadata']['name']}/{c['name']}: requests {requests} != limits {limits}"
                )
    if not (process("executor")["guaranteed"] and process("scheduler")["guaranteed"]):
        problems.append("the executor or the scheduler is not guaranteed")
    return problems


def default_deny(docs: list[dict]) -> list[str]:
    """One policy selects every pod in the namespace and allows nothing, in and out."""
    deny = policies(docs).get("default-deny")
    if deny is None:
        return ["no default-deny policy"]
    spec = deny["spec"]
    if spec != {"podSelector": {}, "policyTypes": ["Ingress", "Egress"]}:
        return [f"default-deny is {spec}"]
    return []


def rule_destinations(rule: dict, cidr_names: dict[str, str]) -> set[tuple[str, str, int]]:
    """Every (destination, protocol, port) one egress rule allows, over all its peers and ports;
    a rule with no ports allows every port, written as port 0."""
    ports = {(p.get("protocol", "TCP"), p.get("port", 0)) for p in rule.get("ports") or [{}]}
    names = set()
    for peer in rule.get("to") or [{}]:
        if "ipBlock" in peer:
            names.add(cidr_names.get(peer["ipBlock"]["cidr"], "cidr:" + peer["ipBlock"]["cidr"]))
        elif "namespaceSelector" in peer:
            names.add("dns")
        elif "podSelector" in peer:
            labels = peer["podSelector"]["matchLabels"]
            names.add(
                "postgres"
                if labels.get("cnpg.io/cluster") == "journal"
                else labels.get("mandate.dev/process", "?")
            )
        else:
            names.add("anywhere")
    return {(n, proto, port) for n in names for proto, port in ports}


DNS = {("dns", "UDP", 53), ("dns", "TCP", 53)}


def egress_exact(docs: list[dict], cidrs: dict[str, str] | None = None) -> list[str]:
    """Each process's egress is exactly its §3.1 column (SECTION_3_1): DNS when it has a
    destination, each in-cluster one by label and port, each external one only through the CIDRs
    values give it; and it accepts no ingress (E21-1, OPS-5)."""
    cidrs = {"kube-api": KUBE_API, **(cidrs or {})}
    names = {c: t for t, c in cidrs.items()}
    problems = []
    found = policies(docs)
    if {p["name"] for p in PROCESSES} != set(SECTION_3_1):
        problems.append(
            f"processes.json lists {sorted(p['name'] for p in PROCESSES)}, §3.1 {sorted(SECTION_3_1)}"
        )
    for name, column in SECTION_3_1.items():
        policy = found.get(f"egress-{name}")
        if policy is None:
            problems.append(f"{name}: no egress policy")
            continue
        spec = policy["spec"]
        if spec["podSelector"]["matchLabels"] != {
            "mandate.dev/workspace": "ws1",
            "mandate.dev/process": name,
        }:
            problems.append(f"{name}: selects {spec['podSelector']}")
        if spec["policyTypes"] != ["Ingress", "Egress"] or spec.get("ingress"):
            problems.append(f"{name}: accepts ingress")
        want = {d for d in column if d[0] in IN_CLUSTER or d[0] in cidrs}
        if column:
            want |= DNS
        got = set().union(*(rule_destinations(r, names) for r in spec.get("egress") or []))
        if got != want:
            problems.append(f"{name}: egress {sorted(got)}, want {sorted(want)}")
    return problems


def no_ingress(docs: list[dict]) -> list[str]:
    """The data plane accepts no inbound connection: the one ingress rule is the model gateway's,
    from the processes that name it."""
    problems = []
    for name, policy in policies(docs).items():
        rules = policy["spec"].get("ingress")
        if not rules:
            continue
        if name != "ingress-model-gateway":
            problems.append(f"{name} accepts ingress")
            continue
        source = rules[0]["from"][0]["podSelector"]["matchLabels"]
        if (
            source != {"mandate.dev/workspace": "ws1", "mandate.dev/egress-model-gateway": "true"}
            or len(rules) != 1
        ):
            problems.append(f"{name} accepts {rules}")
    return problems


def hardened(docs: list[dict]) -> list[str]:
    """Every pod non-root with the runtime's seccomp profile and no service-account token, but the
    deployment manager's, which mounts its own service account's and nothing else (DEC-1172); every
    container read-only, unprivileged, with every capability dropped."""
    problems = []
    for w in workloads(docs):
        spec = pod_spec(w)
        sc = spec.get("securityContext", {})
        token = (
            {"automountServiceAccountToken": True, "serviceAccountName": MANAGER}
            if is_manager(w)
            else {"automountServiceAccountToken": False, "serviceAccountName": None}
        )
        if not (
            sc.get("runAsNonRoot") is True
            and sc.get("runAsUser", 0) != 0
            and sc.get("seccompProfile") == {"type": "RuntimeDefault"}
            and all(spec.get(k) == v for k, v in token.items())
            and not any(spec.get(k) for k in ("hostNetwork", "hostPID", "hostIPC"))
            and not any("hostPath" in v for v in spec.get("volumes", []))
        ):
            problems.append(f"{w['metadata']['name']}: pod {sc}")
        for c in containers(w):
            csc = c.get("securityContext", {})
            if not (
                csc.get("readOnlyRootFilesystem") is True
                and csc.get("allowPrivilegeEscalation") is False
                and csc.get("privileged") is not True
                and csc.get("capabilities", {}).get("drop") == ["ALL"]
                and not csc.get("capabilities", {}).get("add")
            ):
                problems.append(f"{w['metadata']['name']}/{c['name']}: {csc}")
    return problems


def gated(docs: list[dict]) -> list[str]:
    """Every pod's first container is the egress gate, which lets the process start only once the
    pod's own egress is shown filtered, and fails closed after a minute (AGENTS.md rule 3). The gate
    waits on the API server, but the deployment manager's, whose policy allows the API server: it
    waits on the cluster resolver's metrics port, which its policy denies (DEC-1172)."""
    problems = []
    for w in workloads(docs):
        first = (pod_spec(w).get("initContainers") or [{}])[0]
        script = " ".join(first.get("command", []))
        waits_on_api = "KUBERNETES_SERVICE_HOST" in script
        waits_on_resolver = "/etc/resolv.conf" in script and "/9153" in script
        if (
            first.get("name") != "egress-gate"
            or (waits_on_api if is_manager(w) else not waits_on_api)
            or (not waits_on_resolver if is_manager(w) else waits_on_resolver)
            or "exit 1" not in script
        ):
            problems.append(f"{w['metadata']['name']} starts before its egress is shown filtered")
    return problems


SECRET_KEY = re.compile(
    r"(password|passwd|secret|apikey|api[_-]?key|credential|token$|token_|_token)", re.IGNORECASE
)


# Variables that name where a credential file is, never hold one: libpq's pgpass file and systemd's
# credentials directory, which the API server reads its session key from (workspace API §3.11).
FILE_PATH_NAMES = ("PGPASSFILE", "CREDENTIALS_DIRECTORY")


def secrets_as_files(docs: list[dict]) -> list[str]:
    """Secrets arrive only as mounted files (DEC-822 item 4): no env from a Secret, no password in a
    DSN, no Secret object in the chart."""
    problems = [f"the chart renders Secret {d['metadata']['name']}" for d in docs if d["kind"] == "Secret"]
    for w in workloads(docs):
        for c in containers(w):
            if c.get("envFrom"):
                problems.append(f"{c['name']}: envFrom")
            for e in c.get("env", []):
                if "valueFrom" in e:
                    problems.append(f"{c['name']}: {e['name']} from {e['valueFrom']}")
                if re.search(r"://[^/@]*:[^/@]*@", e.get("value", "")):
                    problems.append(f"{c['name']}: {e['name']} carries a password")
                if SECRET_KEY.search(e["name"]) and e["name"] not in FILE_PATH_NAMES:
                    problems.append(f"{c['name']}: {e['name']} looks like a secret")
    return problems


def values_hold_no_secret() -> list[str]:
    problems = []
    for path in [CHART / "values.yaml", JOURNAL_CHART / "values.yaml", *(CELL / "values").glob("*.yaml")]:
        for line in path.read_text().splitlines():
            key = line.split("#")[0].split(":")[0].strip()
            if key and SECRET_KEY.search(key):
                problems.append(f"{path.name}: `{key}`")
    return problems


def migration_first(docs: list[dict]) -> list[str]:
    """Migrations run as a Job under the migration-owner role before processes roll: a pre-install
    and pre-upgrade hook, after its own network policy and its files, and no process migrates."""
    problems = []
    jobs = [d for d in docs if d["kind"] == "Job" and d["metadata"]["name"] == "migrate"]
    if len(jobs) != 1:
        return [f"{len(jobs)} migration Jobs"]
    annotations = jobs[0]["metadata"].get("annotations", {})
    if annotations.get("helm.sh/hook") != HOOK:
        problems.append(f"the migration Job is not a {HOOK} hook: {annotations}")
    weight = int(annotations.get("helm.sh/hook-weight", "0"))
    for name in ("egress-migrate", "migrations"):
        before = [d for d in docs if d["metadata"]["name"] == name]
        ann = before[0]["metadata"].get("annotations", {}) if before else {}
        if ann.get("helm.sh/hook") != HOOK or int(ann.get("helm.sh/hook-weight", "0")) >= weight:
            problems.append(f"{name} is not a hook made before the migration Job")
    dsn = next(e["value"] for e in jobs[0]["spec"]["template"]["spec"]["containers"][0]["env"])
    if not dsn.startswith("postgres://mandate_migrator@"):
        problems.append(f"the migration Job connects as {dsn}")
    for w in workloads(docs):
        if w["metadata"]["name"] == "migrate":
            continue
        for c in containers(w):
            words = " ".join(c.get("command", []) + c.get("args", []))
            if "migrate" in words or "mandate_migrator" in json.dumps(c.get("env", [])):
                problems.append(f"{w['metadata']['name']}/{c['name']} migrates")
    return problems


ALLOWED_APIS = {
    "v1",
    "apps/v1",
    "batch/v1",
    "networking.k8s.io/v1",
    "rbac.authorization.k8s.io/v1",
    "admissionregistration.k8s.io/v1",
}


def portable(docs: list[dict]) -> list[str]:
    """No k3s-only, provider-only or cloud-only API (DEC-967 item 6): core Kubernetes APIs only, no
    storage class, node name or ingress class in the workspace chart. An admission policy's
    validations, which refuse such a field (DEC-1177), name none."""
    problems = [f"{d['kind']} uses {d['apiVersion']}" for d in docs if d["apiVersion"] not in ALLOWED_APIS]
    named = [
        {**d, "spec": {k: v for k, v in d["spec"].items() if k != "validations"}} if d["kind"] == POLICY else d
        for d in docs
    ]
    text = json.dumps(named)
    for word in (
        "storageClassName",
        "nodeName",
        "nodeSelector",
        "ingressClassName",
        "cattle.io",
        "hetzner",
        "digitalocean",
    ):
        if word in text:
            problems.append(f"the chart names {word}")
    return problems


def roles_match(journal_docs: list[dict]) -> list[str]:
    """Every process type's role is created by the roles file, given a login and a password by the
    journal-db chart, and has a credential Secret with a pgpass key."""
    roles = sorted({p["db_role"] for p in PROCESSES if p["db_role"]} - {"mandate_migrator"})
    cluster = next(d for d in journal_docs if d["kind"] == "Cluster")
    managed = sorted(r["name"] for r in cluster["spec"]["managed"]["roles"])
    secrets = {d["metadata"]["name"]: d for d in journal_docs if d["kind"] == "Secret"}
    problems = [] if managed == roles else [f"managed roles {managed}, want {roles}"]
    for role in [*roles, "mandate_migrator"]:
        if (
            role != "mandate_migrator"
            and f"CREATE ROLE {role} LOGIN IN ROLE mandate_journal_app;" not in ROLES_SQL
        ):
            problems.append(f"roles.sql does not create {role}")
        secret = secrets.get("journal-role-" + role.replace("_", "-"))
        if secret is None or not secret["stringData"]["pgpass"].startswith(f"*:*:mandate:{role}:"):
            problems.append(f"no pgpass credential for {role}")
    if cluster["spec"].get("enableSuperuserAccess") is not False:
        problems.append("the journal's superuser is reachable")
    return problems


def compose_matches(compose: dict) -> list[str]:
    """The Compose file holds exactly the built processes, each with the chart's settings: non-root,
    read-only, no capability, no new privileges, secrets as files, the internal network only unless
    it has an external target, and the executor's and scheduler's reservations equal to limits."""
    services = compose["services"]
    built = {p["name"] for p in PROCESSES if p["built"] and p.get("compose", True)}
    problems = (
        []
        if set(services) - {"postgres"} == built
        else [f"services {sorted(services)}, built {sorted(built)}"]
    )
    if MANAGER in services:
        problems.append("the deployment manager is in the Compose file, which it has no adapter for")
    if compose["networks"]["journal"] != {"internal": True}:
        problems.append("the journal network routes out")
    for name in sorted(built & set(services)):
        s, p = services[name], process(name)
        if not (
            s.get("read_only") is True
            and s.get("user") == "999:999"
            and s.get("cap_drop") == ["ALL"]
            and "cap_add" not in s
            and s.get("security_opt") == ["no-new-privileges:true"]
        ):
            problems.append(f"{name} is not hardened")
        for key, value in s.get("environment", {}).items():
            if (SECRET_KEY.search(key) and key not in FILE_PATH_NAMES) or re.search(
                r"://[^/@]*:[^/@]*@", value
            ):
                problems.append(f"{name}: {key} holds a secret")
        external = any(t not in IN_CLUSTER for t in p["egress"])
        if ("egress" in s["networks"]) != external:
            problems.append(f"{name}: networks {s['networks']}")
        res = s["deploy"]["resources"]
        if p["guaranteed"] and res["limits"] != res["reservations"]:
            problems.append(f"{name}: reservations {res['reservations']} != limits {res['limits']}")
    return problems


def test_one_workload_per_process():
    assert one_workload_per_process(render_all()) == []


CONNECT_CHECK = ["--set", "run.process=connect-check"]


def test_a_run_renders_only_the_process_it_names():
    """A one-off run renders one Job, of `control` unless `run.process` names another run process;
    a name that is no run process fails the render rather than running nothing (DEC-2035)."""
    assert one_workload_per_process(render_all(*CONNECT_CHECK), "connect-check") == []
    assert (
        helm(
            CHART,
            "-f",
            str(CELL / "values" / "k3d.yaml"),
            *every_image(),
            "--set",
            "run.id=t1",
            "--set",
            "run.process=runtime",
        ).returncode
        != 0
    )


def test_the_connect_check_runs_the_executor_on_its_journal_role_alone():
    """The connect check is the executor's image, run with the run's arguments as the executor's
    journal role, with Postgres its one destination and no store: its broker is the in-process
    fixture (DEC-2007, DEC-2035)."""
    docs = render_all(*CONNECT_CHECK)
    job = next(
        w for w in workloads(docs) if w["metadata"]["labels"].get("mandate.dev/process") == "connect-check"
    )
    run = pod_spec(job)["containers"][0]
    assert run["image"] == PLACEHOLDER.format("executor")
    assert run["args"] == ["workspace", "open", "--workspace", "ws9"]
    dsn = next(e["value"] for e in run["env"] if e["name"] == "MANDATE_JOURNAL_DSN")
    assert dsn.startswith("postgres://mandate_executor@")
    assert "artifacts" not in {v["name"] for v in pod_spec(job)["volumes"]}
    for check in (
        colocated,
        guaranteed,
        default_deny,
        no_ingress,
        gated,
        hardened,
        secrets_as_files,
        migration_first,
    ):
        assert check(docs) == [], check.__name__
    assert egress_exact(docs) == []


def test_a_process_renders_only_with_its_image():
    docs = render("-f", str(CELL / "values" / "k3d.yaml"))
    assert {w["metadata"]["name"] for w in workloads(docs)} == {"migrate"}
    assert set(policies(docs)) == {"default-deny", "egress-migrate"}


def test_each_service_starts_and_is_probed_as_its_process_list_says():
    """A service with `args` starts with them and is asked `readyz` and `livez` with its `probe`, every
    placeholder filled from the values (DEC-1013, DEC-994 item 5). Without the deployment manager, so
the processes it starts render as Deployments of their own; its pod templates are
`test_deploy_manager_contained`'s."""
    docs = render_all("--set", f"images.{MANAGER}=")
    for p in PROCESSES:
        if p["kind"] != "service" or not (p.get("args") or p.get("probe")) or p.get("one_shot"):
            continue
        w = next(w for w in workloads(docs) if w["metadata"]["name"] == p["name"])
        c = first_container(w)
        probes = [x for k in ("readinessProbe", "livenessProbe") for x in c[k]["exec"]["command"]]
        words = c.get("args", []) + probes
        assert not [x for x in words if "{" in x], words
        assert c["args"][: len(p["args"])] == [
            x.replace("{workspace}", "ws1")
            .replace("{agent}", "demo-agent")
            .replace("{connection}", "demo-paper")
            .replace("{owner}", "demo-owner")
            for x in p["args"]
        ]
        assert c["readinessProbe"]["exec"]["command"] == [x.replace("{probe}", "readyz") for x in p["probe"]]
        assert c["livenessProbe"]["exec"]["command"] == [x.replace("{probe}", "livez") for x in p["probe"]]


def test_the_runtime_never_renders_for_no_deployment():
    """Naming the runtime's image without the deployment it runs fails the render, rather than
    starting a runtime for no agent (DEC-1013, AGENTS.md rule 3)."""
    done = helm(CHART, "--set", "images.runtime=mandate.local/runtime:0123456789ab")
    assert done.returncode != 0
    assert "runtime.agent is required" in done.stderr


def test_colocated():
    assert colocated(render_all()) == []


def test_guaranteed():
    assert guaranteed(render_all()) == []


def test_default_deny():
    assert default_deny(render_all()) == []


def test_egress_exact():
    assert egress_exact(render_all()) == []


def test_each_external_target_reaches_only_its_processes():
    names = ["broker", "token-host", "time", "model-providers", "object-storage", "timestamping"]
    names += ["artifact-store", "identity-provider"]
    cidrs = {t: f"192.0.2.{i}/32" for i, t in enumerate(names, start=1)}
    cidrs["market-data"] = "198.51.100.1/32"
    cidrs["kube-api"] = "198.51.100.2/32"
    extra = [arg for t, c in cidrs.items() for arg in ("--set-json", f'egress.{t}=["{c}"]')]
    docs = render_all(*extra)
    assert egress_exact(docs, cidrs) == []
    token = [n for n, p in policies(docs).items() if "192.0.2.2/32" in json.dumps(p)]
    assert token == ["egress-token-exchange"], "DEC-821's token host is the token-exchange process's alone"
    api = [n for n, p in policies(docs).items() if "198.51.100.2/32" in json.dumps(p)]
    assert api == [f"egress-{MANAGER}"], "the API server is the deployment manager's alone (DEC-1172)"


def test_no_ingress():
    assert no_ingress(render_all()) == []


def test_gated():
    assert gated(render_all()) == []


def test_hardened():
    assert hardened(render_all()) == []


def test_secrets_as_files():
    assert secrets_as_files(render_all()) == []
    assert values_hold_no_secret() == []


def test_migration_first():
    assert migration_first(render_all()) == []


LOOPBACK = re.compile(r"^(127\.0\.0\.1|\[::1\]):[0-9]+$")
SUBJECT_LINKS = "/etc/mandate/subject-links"


def api_contained(docs: list[dict]) -> list[str]:
    """The API server (workspace API §3.11) listens on its pod's loopback only, so nothing in the
    cluster reaches it before the tunnel's route; its session key lives only in a memory-backed
    directory it reads but cannot write, made owner-only at each start; and the subject links, when
    named, are a ConfigMap mounted read-only at the one fixed path (DEC-1023)."""
    problems = []
    for w in [w for w in workloads(docs) if w["metadata"]["name"] == "api"]:
        spec = pod_spec(w)
        api = next(c for c in spec["containers"] if c["name"] == "api")
        args = api.get("args", [])
        binds = [args[i + 1] for i, a in enumerate(args[:-1]) if a in ("--bind", "--probe-bind")]
        if sorted(a for a in args if a.startswith("--")) != ["--bind", "--probe-bind"] or not all(
            LOOPBACK.match(b) for b in binds
        ):
            problems.append(f"api listens beyond its loopback: {args}")
        env = {e["name"]: e.get("value") for e in api.get("env", [])}
        volumes = {v["name"]: v for v in spec.get("volumes", [])}
        mounts = {m["mountPath"]: m for m in api.get("volumeMounts", [])}
        key_dir = env.get("CREDENTIALS_DIRECTORY")
        key_mount = mounts.get(key_dir or "")
        key_volume = volumes.get((key_mount or {}).get("name", ""), {})
        if not (
            key_mount
            and key_mount.get("readOnly") is True
            and key_volume.get("emptyDir", {}).get("medium") == "Memory"
        ):
            problems.append(f"api's session key is not in read-only pod memory: {key_mount}, {key_volume}")
        makers = [
            c
            for c in spec.get("initContainers", [])
            if any(m["name"] == (key_mount or {}).get("name") for m in c.get("volumeMounts", []))
        ]
        if len(makers) != 1 or "umask 077" not in " ".join(makers[0].get("command", [])):
            problems.append("api's session key is not made owner-only by one init container")
        links = mounts.get(SUBJECT_LINKS)
        if links is not None:
            source = volumes.get(links["name"], {})
            if links.get("readOnly") is not True or not source.get("configMap") or "emptyDir" in source:
                problems.append(f"api's subject links are not a read-only ConfigMap: {links}, {source}")
        if any(m.startswith("/etc/mandate") and m != SUBJECT_LINKS for m in mounts):
            problems.append(f"api mounts another file under /etc/mandate: {sorted(mounts)}")
    return problems


def test_api_contained():
    plain = render_all()
    linked = render_all("--set", "api.subjectLinks=subject-links")
    assert [w["metadata"]["name"] for w in workloads(plain) if w["metadata"]["name"] == "api"] == ["api"]
    assert api_contained(plain) == [] and api_contained(linked) == []
    assert SUBJECT_LINKS not in {
        m["mountPath"] for m in first_container(owner_workload(plain))["volumeMounts"]
    }
    assert SUBJECT_LINKS in {m["mountPath"] for m in first_container(owner_workload(linked))["volumeMounts"]}
    for seed in (
        lambda w: first_container(w)["volumeMounts"][-1].update(readOnly=False),
        lambda w: pod_spec(w)["volumes"][-1].update(configMap=None, emptyDir={}),
        lambda w: first_container(w)["volumeMounts"].append(
            {"name": "tmp", "mountPath": "/etc/mandate/other"}
        ),
    ):
        assert api_contained(mutate(linked, "api", "Deployment", seed)), seed
    for refused in ("api.org=", "api.workspace=", "api.jwksUrl=http://issuer.example/jwks"):
        args = ["-f", str(CELL / "values" / "k3d.yaml"), *every_image(), "--set", refused]
        assert helm(CHART, *args).returncode != 0, refused


def api_env(rendered: list[dict]) -> dict[str, str]:
    """The api container's environment, name to value."""
    return {e["name"]: e.get("value", "") for e in first_container(owner_workload(rendered)).get("env", [])}


def test_ingress_peers_are_exact_addresses_set_only_when_named():
    """Route 2's ingress peers (workspace API spec §4.10, DEC-2119 item 11): no variable when the
    list is empty, the comma-joined list otherwise, and only exact addresses admitted, IPv4 with
    each octet 0 to 255, IPv6, or IPv4-mapped IPv6."""
    assert "MANDATE_API_INGRESS_PEERS" not in api_env(render_all())
    peers = render_all("--set-json", 'api.ingressPeers=["127.0.0.1","::ffff:127.0.0.1","::1"]')
    assert api_env(peers)["MANDATE_API_INGRESS_PEERS"] == "127.0.0.1,::ffff:127.0.0.1,::1"
    base = ["-f", str(CELL / "values" / "k3d.yaml"), *every_image()]
    for refused in ('["999.1.1.1"]', '["256.0.0.1"]', '["10.0.0.0/8"]', '["localhost"]', '["127.0.0.1","127.0.0.1"]'):
        assert helm(CHART, *base, "--set-json", f"api.ingressPeers={refused}").returncode != 0, refused


FOUNDER = "01KDEM0000000000000000000F"
FOUNDING = ("--set", "founding.enabled=true", "--set", f"founding.member={FOUNDER}")
FOUNDING_HOOK = "post-install,post-upgrade"


def founding_hook(docs: list[dict]) -> list[str]:
    """With founding on (DEC-2114), the one `founding` Job is the control process's: a post-install
    and post-upgrade hook made again before each run, running exactly `workspace found --open` for
    the founder on the hosted workspace as the control role, and naming no identity-provider
    subject anywhere."""
    jobs = [d for d in docs if d["kind"] == "Job" and d["metadata"]["name"] == "founding"]
    if len(jobs) != 1:
        return [f"{len(jobs)} founding Jobs"]
    job = jobs[0]
    problems = []
    annotations = job["metadata"].get("annotations", {})
    if annotations.get("helm.sh/hook") != FOUNDING_HOOK:
        problems.append(f"the founding Job is not a {FOUNDING_HOOK} hook: {annotations}")
    if annotations.get("helm.sh/hook-delete-policy") != "before-hook-creation":
        problems.append(f"the founding Job is not made again before each run: {annotations}")
    if job["metadata"]["labels"].get("mandate.dev/process") != "control":
        problems.append(f"the founding Job is not the control process's: {job['metadata']['labels']}")
    run = pod_spec(job)["containers"]
    if len(run) != 1:
        return [*problems, f"the founding Job runs {len(run)} containers"]
    args = run[0].get("args", [])
    want = [
        "workspace",
        "found",
        "--open",
        "--workspace",
        args[4] if len(args) > 4 else "",
        "--member",
        FOUNDER,
        "--method",
        "oidc",
        "--journal",
        "$(MANDATE_JOURNAL_DSN)",
        "--store",
        "/work/store",
    ]
    if args != want or not re.fullmatch(r"[0-7][0-9A-HJKMNP-TV-Z]{25}", want[4]):
        problems.append(f"the founding Job runs {args}")
    dsn = next((e.get("value", "") for e in run[0]["env"] if e["name"] == "MANDATE_JOURNAL_DSN"), "")
    if not dsn.startswith("postgres://mandate_control@"):
        problems.append(f"the founding Job connects as {dsn}")
    if "subject" in json.dumps(job).lower():
        problems.append("the founding Job names a subject")
    return problems


def test_the_founding_grant_is_a_hook_of_the_control_process():
    """Off by default; on, one hook Job of `control` with control's role, gate, hardening, files
    and egress, and nothing else changed; refused without the workspace, the member, or the control
    process's image (DEC-2114)."""
    assert not [d for d in render_all() if d["metadata"]["name"] == "founding"]
    base = ("-f", str(CELL / "values" / "k3d.yaml"), *every_image(), "--set-json", f'egress.kube-api=["{KUBE_API}"]')
    docs = render(*base, *FOUNDING)
    plain = render(*base)
    assert founding_hook(docs) == []
    assert [d for d in docs if d["metadata"]["name"] != "founding"] == plain, "founding adds only its Job"
    for check in (gated, hardened, secrets_as_files, egress_exact):
        assert check(docs) == [], check.__name__
    for seed in (
        lambda j: j["metadata"]["annotations"].update({"helm.sh/hook": "pre-install,pre-upgrade"}),
        lambda j: j["metadata"]["annotations"].pop("helm.sh/hook-delete-policy"),
        lambda j: j["metadata"]["labels"].update({"mandate.dev/process": "api"}),
        lambda j: pod_spec(j)["containers"][0]["args"].remove("--open"),
        lambda j: pod_spec(j)["containers"][0]["args"].append("--subject"),
        lambda j: pod_spec(j)["containers"][0]["env"].append({"name": "FOUNDER_OIDC_SUBJECT", "value": "x"}),
        lambda j: pod_spec(j)["containers"][0]["env"][0].update(value="postgres://mandate_migrator@journal-rw"),
        lambda j: pod_spec(j)["containers"].append(pod_spec(j)["containers"][0]),
    ):
        assert founding_hook(mutate(docs, "founding", "Job", seed)), seed
    assert founding_hook([d for d in docs if d["metadata"]["name"] != "founding"])
    for refused in (
        ("--set", "founding.member="),
        ("--set", "api.workspace="),
        ("--set", "founding.member=not-a-ulid"),
    ):
        args = ["-f", str(CELL / "values" / "k3d.yaml"), *every_image(), *FOUNDING, *refused]
        assert helm(CHART, *args).returncode != 0, refused
    without_control = ["-f", str(CELL / "values" / "k3d.yaml"), *FOUNDING]
    assert helm(CHART, *without_control).returncode != 0


# The public CA bundle the API server trusts for the identity provider's key set (RS's finding 1):
# copied from the pinned postgres image by the pod's init container, since the API's own image is
# a bare binary with none.
CA_BUNDLE_DIR = "/run/ca-bundle"
CA_BUNDLE = f"{CA_BUNDLE_DIR}/ca-certificates.crt"
SYSTEM_BUNDLE = "/etc/ssl/certs/ca-certificates.crt"


def ca_bundle(docs: list[dict]) -> list[str]:
    """The API container, and no other container, mounts the CA bundle read-only and names it in
    SSL_CERT_FILE; one init container of its pod copies it from the system bundle."""
    problems = []
    for w in workloads(docs):
        spec = pod_spec(w)
        for c in spec.get("containers", []):
            env = {e["name"]: e.get("value") for e in c.get("env", [])}
            mounts = {m["mountPath"]: m for m in c.get("volumeMounts", [])}
            is_api = w["metadata"]["name"] == "api" and c["name"] == "api"
            if not is_api:
                if "SSL_CERT_FILE" in env or CA_BUNDLE_DIR in mounts:
                    problems.append(f"{w['metadata']['name']}/{c['name']} gets the API's CA bundle")
                continue
            mount = mounts.get(CA_BUNDLE_DIR)
            if env.get("SSL_CERT_FILE") != CA_BUNDLE:
                problems.append(f"api's SSL_CERT_FILE is {env.get('SSL_CERT_FILE')!r}")
            if not mount or mount.get("readOnly") is not True:
                problems.append(f"api does not mount the CA bundle read-only: {mount}")
                continue
            copiers = [
                i
                for i in spec.get("initContainers", [])
                if any(m["name"] == mount["name"] for m in i.get("volumeMounts", []))
                and f"cp {SYSTEM_BUNDLE} {CA_BUNDLE}" in " ".join(i.get("command", []))
            ]
            if len(copiers) != 1:
                problems.append("api's CA bundle is not copied by one init container")
    return problems


def test_only_the_api_gets_the_ca_bundle():
    docs = render_all()
    assert ca_bundle(docs) == []
    api = owner_workload(docs)
    assert CA_BUNDLE_DIR in {m["mountPath"] for m in first_container(api)["volumeMounts"]}
    for seed in (
        lambda w: next(m for m in first_container(w)["volumeMounts"] if m["mountPath"] == CA_BUNDLE_DIR).update(
            readOnly=False
        ),
        lambda w: first_container(w).update(
            env=[e for e in first_container(w)["env"] if e["name"] != "SSL_CERT_FILE"]
        ),
        lambda w: pod_spec(w).update(initContainers=[]),
    ):
        assert ca_bundle(mutate(docs, "api", "Deployment", seed)), seed
    other = next(w for w in workloads(docs) if w["kind"] == "Deployment" and w["metadata"]["name"] != "api")
    leaked = mutate(
        docs,
        other["metadata"]["name"],
        other["kind"],
        lambda w: first_container(w).setdefault("env", []).append({"name": "SSL_CERT_FILE", "value": CA_BUNDLE}),
    )
    assert ca_bundle(leaked)


POD_RULE = {"apiGroups": [""], "resources": ["pods"], "verbs": ["create", "delete", "get", "list"]}
# The ServiceAccount the manager makes for a connection's token exchange (DEC-1177): the same four
# verbs, so it can create one before its pod and delete it at the close.
ACCOUNT_RULE = {"apiGroups": [""], "resources": ["serviceaccounts"], "verbs": ["create", "delete", "get", "list"]}
TOKEN_PATH = "/var/run/secrets/kubernetes.io/serviceaccount"


def mounts_token(spec: dict) -> bool:
    """Whether a pod gets a service-account token: Kubernetes mounts one unless the pod says not to."""
    return spec.get("automountServiceAccountToken") is not False or bool(spec.get("serviceAccountName"))


def deploy_manager_contained(docs: list[dict]) -> list[str]:
    """The deployment manager is the one pod with access to the API server (DEC-1172): one Role in
    its namespace whose single rule creates, reads, lists and deletes pods in the core group; no
    cluster-scoped grant; one binding, of that Role to its own ServiceAccount alone; no other pod,
    and none of the pod templates it starts, mounts a service-account token; no other policy reaches
    the API server; its gate waits on a destination its policy denies, never the API server; and it
    reads its templates from its ConfigMap, read-only."""
    problems = []
    for kind in ("ClusterRole", "ClusterRoleBinding"):
        problems += [f"{kind} {d['metadata']['name']}" for d in docs if d["kind"] == kind]
    managers = [w for w in workloads(docs) if is_manager(w)]
    if not managers:
        grants = [
            d
            for d in docs
            if d["kind"] in ("ServiceAccount", "Role", "RoleBinding", "ValidatingAdmissionPolicy")
        ]
        problems += [f"{d['kind']} {d['metadata']['name']} without the manager" for d in grants]
    roles = [d for d in docs if d["kind"] == "Role"]
    bindings = [d for d in docs if d["kind"] == "RoleBinding"]
    accounts = [d for d in docs if d["kind"] == "ServiceAccount"]
    if managers:
        rules = [{k: sorted(v) for k, v in r.items()} for role in roles for r in role.get("rules") or []]
        if [r["metadata"]["name"] for r in roles] != [MANAGER] or rules != [POD_RULE, ACCOUNT_RULE]:
            problems.append(
                f"the manager's Role is not exactly pods' and ServiceAccounts' create, get, list, delete: {rules}"
            )
        if [a["metadata"]["name"] for a in accounts] != [MANAGER]:
            problems.append(f"service accounts {[a['metadata']['name'] for a in accounts]}")
        binds = [(b.get("roleRef"), b.get("subjects")) for b in bindings]
        want = (
            {"apiGroup": "rbac.authorization.k8s.io", "kind": "Role", "name": MANAGER},
            [{"kind": "ServiceAccount", "name": MANAGER, "namespace": "ws-ws1"}],
        )
        if binds != [want]:
            problems.append(f"role bindings {binds}")
    for w in workloads(docs):
        spec = pod_spec(w)
        name = f"{w['kind']} {w['metadata']['name']}"
        if is_manager(w):
            if (
                spec.get("automountServiceAccountToken") is not True
                or spec.get("serviceAccountName") != MANAGER
            ):
                problems.append(f"{name} does not mount its own service account's token")
        elif mounts_token(spec):
            problems.append(f"{name} mounts a service-account token")
        if any(
            m["mountPath"].startswith(TOKEN_PATH) for c in containers(w) for m in c.get("volumeMounts", [])
        ):
            problems.append(f"{name} mounts a token by hand")
        labels = w["spec"]["template"]["metadata"]["labels"]
        if labels.get("mandate.dev/egress-kube-api") and not is_manager(w):
            problems.append(f"{name} is labelled for the API server")
    for policy_name, policy in policies(docs).items():
        for rule in policy["spec"].get("egress") or []:
            api = [
                peer
                for peer in rule.get("to") or [{}]
                if peer.get("ipBlock", {}).get("cidr") == KUBE_API or not peer
            ]
            if api and policy_name != f"egress-{MANAGER}":
                problems.append(f"{policy_name} reaches the API server: {rule}")
    for w in managers:
        spec = pod_spec(w)
        gate = (spec.get("initContainers") or [{}])[0]
        script = " ".join(gate.get("command", []))
        probed = re.findall(r"/dev/tcp/\$([A-Za-z_]+)/(\$?[A-Za-z_0-9]+)", script)
        allowed = set().union(
            *(
                rule_destinations(r, {KUBE_API: "kube-api"})
                for r in policies(docs)[f"egress-{MANAGER}"]["spec"]["egress"]
            )
        )
        resolver = 'awk \'$1 == "nameserver"' in script and "/etc/resolv.conf" in script
        if (
            gate.get("name") != "egress-gate"
            or probed != [("dns", "9153")]
            or not resolver
            or ("dns", "TCP", 9153) in allowed
            or "KUBERNETES_SERVICE" in script
        ):
            problems.append(f"the manager's gate does not wait on a destination its policy denies: {probed}")
        main = spec["containers"][0]
        templates = [m for m in main.get("volumeMounts", []) if m["mountPath"] == "/etc/mandate/templates"]
        volumes = {v["name"]: v for v in spec.get("volumes", [])}
        source = volumes.get(templates[0]["name"], {}) if templates else {}
        if not (
            templates
            and templates[0].get("readOnly") is True
            and source.get("configMap", {}).get("name") == TEMPLATES
        ):
            problems.append(
                f"the manager's templates are not its ConfigMap, read-only: {templates}, {source}"
            )
        if main.get("args") != [
            "run",
            "--workspace",
            "01KDEM0000000000000000000W",
            "--probe",
            "/tmp/process.sock",
            "--templates",
            "/etc/mandate/templates",
        ]:
            problems.append(f"the manager runs {main.get('args')}")
    return problems


def test_deploy_manager_contained():
    docs = render_all()
    assert deploy_manager_contained(docs) == []
    assert sorted(w["metadata"]["name"] for w in managed_pods(docs)) == ["executor", "runtime", EXCHANGE]
    for pod in managed_pods(docs):
        spec = pod_spec(pod)
        container = first_container(pod)
        if pod["metadata"]["name"] == EXCHANGE:
            assert container["command"] == ["/mandate-token-exchange"], container.get("command")
            assert "args" not in container, container.get("args")
            assert "livenessProbe" not in container and "readinessProbe" not in container
            assert spec["restartPolicy"] == "Never" and spec["automountServiceAccountToken"] is False
            assert "serviceAccountName" not in spec
            continue
        assert "command" not in container, container.get("command")
        research = ["--research", RESEARCH] if pod["metadata"]["name"] == "runtime" else []
        assert container["args"] == ["run", "--probe", "/tmp/process.sock", *research], container["args"]
        assert spec["restartPolicy"] == "Never" and spec["automountServiceAccountToken"] is False
        assert "serviceAccountName" not in spec
        probes = [container[k]["exec"]["command"] for k in ("livenessProbe", "readinessProbe")]
        listed = next(p for p in PROCESSES if p["name"] == pod["metadata"]["name"])
        words = listed.get("probe") or [
            "/mandate-daemon", "probe", "{probe}", "--probe", "/tmp/process.sock"
        ]
        assert probes == [[x.replace("{probe}", k) for x in words] for k in ("livez", "readyz")], probes
    only_runtime = render_all("--set", "images.executor=", "--set", f"images.{EXCHANGE}=")
    assert [w["metadata"]["name"] for w in managed_pods(only_runtime)] == ["runtime"]
    assert deploy_manager_contained(only_runtime) == []
    deny_all = render_all("--set", "runtime.research=")
    assert deploy_manager_contained(deny_all) == []
    assert all(
        first_container(pod)["args"] == ["run", "--probe", "/tmp/process.sock"]
        for pod in managed_pods(deny_all)
        if pod["metadata"]["name"] != EXCHANGE
    )


def test_runtime_research_names_only_the_interim_image_s_bundle():
    """DEC-2246: `runtime.research` is empty or the one path the interim runtime image carries
    the recorded bundle at; any other path is refused by the schema, so no pod is pointed at a
    bundle the image does not hold."""
    k3d = str(CELL / "values" / "k3d.yaml")
    for path in ["/tmp/research", "relative/research", f"{RESEARCH}/"]:
        done = helm(CHART, "-f", k3d, *every_image(), "--set", f"runtime.research={path}")
        assert done.returncode != 0, path


# The executor on Alpaca's paper host (DEC-2306 item 1): staging's tier, and the cell's vault.
VAULT = "https://vault.ws-ws1.svc:8200"
ALPACA_PAPER = [
    "--set", "deploymentTier=staging",
    "--set", "executor.broker=alpaca-paper",
    "--set", f"executor.vaultAddress={VAULT}",
]
PAPER_ARGS = ["--broker", "alpaca-paper", "--vault-address", VAULT, "--vault-dir", "/run/mandate/vault"]


def test_the_executor_runs_on_alpaca_paper_where_staging_names_it():
    """DEC-2306 item 1: `executor.broker=alpaca-paper` hands every executor the paper host, the
    vault's address and the CA's directory, whether the manager starts it or it is a Deployment,
    and nothing else changes: the runtime's args, and the manager's containment."""
    docs = render_all(*ALPACA_PAPER)
    assert deploy_manager_contained(docs) == []
    args = {p["metadata"]["name"]: first_container(p)["args"] for p in managed_pods(docs) if p["metadata"]["name"] != EXCHANGE}
    assert args == {
        "executor": ["run", "--probe", "/tmp/process.sock", *PAPER_ARGS],
        "runtime": ["run", "--probe", "/tmp/process.sock", "--research", RESEARCH],
    }, args
    unmanaged = render_all(*ALPACA_PAPER, "--set", f"images.{MANAGER}=")
    executor = next(
        w for w in workloads(unmanaged) if w["kind"] == "Deployment" and w["metadata"]["name"] == "executor"
    )
    assert first_container(executor)["args"] == ["run", "--probe", "/tmp/process.sock", *PAPER_ARGS]


def test_the_paper_broker_needs_staging_and_its_vault():
    """DEC-2306 item 1: the fixture stays the default and the only broker outside staging; the
    paper host needs the cell's vault by an https address; any other broker, a live one included,
    is refused by the schema, so no executor is pointed at a host the decision does not name."""
    k3d = str(CELL / "values" / "k3d.yaml")
    base = ["-f", k3d, *every_image()]
    assert helm(CHART, *base, *ALPACA_PAPER).returncode == 0
    staging = ALPACA_PAPER[:2]
    refused = {
        "outside staging": ALPACA_PAPER[2:],
        "in production": [*ALPACA_PAPER, "--set", "deploymentTier=production"],
        "without a vault": [*staging, "--set", "executor.broker=alpaca-paper"],
        "over http": [*ALPACA_PAPER, "--set", "executor.vaultAddress=http://vault.ws-ws1.svc:8200"],
        "with a path": [*ALPACA_PAPER, "--set", f"executor.vaultAddress={VAULT}/v1"],
        "a live broker": [*staging, "--set", "executor.broker=alpaca-live"],
        "an unnamed broker": [*staging, "--set", "executor.broker=alpaca"],
    }
    for why, extra in refused.items():
        assert helm(CHART, *base, *extra).returncode != 0, why


def test_without_the_manager_the_runtime_and_executor_are_deployments():
    docs = render_all("--set", f"images.{MANAGER}=")
    kinds = {w["metadata"]["name"]: w["kind"] for w in workloads(docs)}
    assert kinds["runtime"] == kinds["executor"] == "Deployment" and MANAGER not in kinds
    assert not [
        d
        for d in docs
        if d["kind"] in ("ServiceAccount", "Role", "RoleBinding", "ConfigMap")
        and d["metadata"]["name"] != "migrations"
    ]
    for check in (hardened, gated, deploy_manager_contained):
        assert check(docs) == [], check.__name__


def test_the_api_server_is_narrow_and_empty_by_default():
    plain = render(
        "-f", str(CELL / "values" / "k3d.yaml"), "--set", f"images.{MANAGER}={PLACEHOLDER.format(MANAGER)}"
    )
    rules = policies(plain)[f"egress-{MANAGER}"]["spec"]["egress"]
    assert not [r for r in rules if any("ipBlock" in p for p in r.get("to", []))], (
        "no API address until values give one"
    )
    for cidr in ("0.0.0.0/0", "10.0.0.0/8", "192.0.2.1"):
        assert helm(CHART, "--set-json", f'egress.kube-api=["{cidr}"]').returncode != 0, cidr
    assert helm(CHART, "--set", "egress.kubeApiPort=0").returncode != 0
    port = render_all("--set", "egress.kubeApiPort=443")
    rule = [r for r in policies(port)[f"egress-{MANAGER}"]["spec"]["egress"] if "ipBlock" in r["to"][0]]
    assert rule == [{"to": [{"ipBlock": {"cidr": KUBE_API}}], "ports": [{"protocol": "TCP", "port": 443}]}]


POLICY = "ValidatingAdmissionPolicy"
POLICY_BINDING = "ValidatingAdmissionPolicyBinding"
MANAGER_USER = f"system:serviceaccount:ws-ws1:{MANAGER}"
POD_RULES = [
    {"apiGroups": [""], "apiVersions": ["v1"], "operations": ["CREATE", "UPDATE"], "resources": ["pods"]}
]
ACCOUNT_RULES = [
    {
        "apiGroups": [""],
        "apiVersions": ["v1"],
        "operations": ["CREATE", "UPDATE", "DELETE"],
        "resources": ["serviceaccounts"],
    }
]
PODS_POLICY = "-deploy-manager-pods"
ACCOUNTS_POLICY = "-deploy-manager-accounts"
NAMESPACE_SELECTOR = {"matchLabels": {"kubernetes.io/metadata.name": "ws-ws1"}}
# The labels the manager's adapter adds to a template (crates/mandate-deploy-manager kubernetes.rs).
ADDED_LABELS = {
    "app.kubernetes.io/managed-by": "mandate-deploy-manager",
    "mandate.dev/workspace-key": "0123456789abcdef",
    "mandate.dev/generation": "1",
    "mandate.dev/subject-key": "fedcba9876543210",
}


def all_containers(pod: dict) -> list[dict]:
    return [*pod["spec"].get("initContainers", []), *pod["spec"]["containers"]]


def egress_labels(labels: dict) -> set[str]:
    return {k for k in labels if k.startswith("mandate.dev/egress-")}


def is_true(spec: dict, key: str) -> bool:
    return spec.get(key) is True


EXCHANGE = "token-exchange"
PROCESS_VARIABLE = (
    'has(object.metadata.labels) && "mandate.dev/process" in object.metadata.labels'
    ' ? object.metadata.labels["mandate.dev/process"] : ""'
)
# The connection a test's token exchange is for, and the account DEC-2060 names for it: `x` and the
# id's bytes in lowercase hex.
CONNECTION = "conn_01J8Z2AB"
CONNECTION_ACCOUNT = "x" + CONNECTION.encode().hex()
# The evaluator the tests run the rendered policies' CEL on: cel-python, pinned in python/pyproject
# (DEC-1177), so a loosened expression is caught by what it admits, not by how it reads.
CEL = celpy.Environment()


@functools.cache
def cel_program(expression: str):
    return CEL.program(CEL.compile(expression))


def cel_holds(expression: str, activation: dict) -> bool:
    """Whether `expression` is `true`; an error refuses, as the policies' `failurePolicy: Fail` does."""
    try:
        result = cel_program(expression).evaluate(activation)
    except Exception:  # noqa: BLE001
        return False
    return isinstance(result, celtypes.BoolType) and bool(result)


@functools.cache
def cel_literal(expression: str):
    """A variable that reads no request, evaluated once: the policies' per-process literals."""
    return cel_program(expression).evaluate({})


def cel_refusals(
    policy: dict, obj: dict | None, old: dict | None = None, operation: str = "CREATE", first: bool = False
) -> list[str]:
    """The messages of the policy's validations that refuse `obj` (or, on a delete, `old`) in a
    request by the manager, evaluated as the API server evaluates them: match conditions, then the
    variables in order, then each validation; with `first`, only the first that refuses."""
    spec = policy["spec"]
    activation = {
        "object": celpy.json_to_cel(obj),
        "oldObject": celpy.json_to_cel(old),
        "request": celpy.json_to_cel({"operation": operation, "userInfo": {"username": MANAGER_USER}}),
    }
    if not all(cel_holds(c["expression"], activation) for c in spec.get("matchConditions", [])):
        return []
    variables = {}
    for v in spec.get("variables", []):
        reads = any(word in v["expression"] for word in ("object", "request", "variables"))
        activation["variables"] = celtypes.MapType(variables)
        try:
            value = cel_program(v["expression"]).evaluate(activation) if reads else cel_literal(v["expression"])
        except Exception:  # noqa: BLE001
            return [r["message"] for r in spec.get("validations", [])]
        variables[celtypes.StringType(v["name"])] = value
    activation["variables"] = celtypes.MapType(variables)
    refused = []
    for r in spec.get("validations", []):
        if not cel_holds(r["expression"], activation):
            refused.append(r["message"])
            if first:
                break
    return refused


def process_of(pod: dict) -> str:
    return pod["metadata"].get("labels", {}).get("mandate.dev/process", "")


def launches(pod: dict) -> dict:
    """Each container's launch, by name, as the policy's `commands` holds it: `[command, args]`, a
    list of string lists, so every literal in the variable has one type and CEL compiles it."""
    return {c["name"]: [c.get("command", []), c.get("args", [])] for c in all_containers(pod)}


# Each bound the admission policy must hold (DEC-1172 item 4, DEC-1177), by its validation's
# message, and the words its CEL must name. What each admits is the policy's CEL itself, evaluated
# (`cel_refusals`), so a rule that is dropped or loosened, or a list that is widened, admits a pod
# the attacks below must not get.
BOUNDS = {
    "a pod the deployment manager makes mounts no service-account token": [
        "object.spec.automountServiceAccountToken == false"
    ],
    "a pod the deployment manager makes names no service account but the default, or a token exchange its own": [
        "serviceAccountName",
        'object.spec.serviceAccountName == "default"',
        '"token-exchange"',
        "matches('^x([0-9a-f]{2}){1,64}$')",
    ],
    "a token exchange's pod names its own account in its environment once, beside its connection, which the exchange checks against each other": [
        'e.name == "MANDATE_SUBJECT_ACCOUNT"',
        "e.value == object.spec.serviceAccountName",
    ],
    "a pod the deployment manager makes runs as no root user, adds no capability, and names no node or host alias": [
        "runAsNonRoot == true",
        "runAsUser != 0",
        "!has(c.securityContext.capabilities.add)",
        "!has(object.spec.hostAliases)",
        "!has(object.spec.nodeName)",
    ],
    "a pod the deployment manager makes shares no host namespace": ["hostNetwork", "hostPID", "hostIPC"],
    "a pod the deployment manager makes mounts no host path": ["!has(v.hostPath)"],
    "every container of a pod the deployment manager makes is unprivileged, with no privilege escalation": [
        "object.spec.initContainers.all(c,",
        "allowPrivilegeEscalation == false",
        "privileged == false",
    ],
    "a pod the deployment manager makes has no ephemeral container": ["!has(object.spec.ephemeralContainers)"],
    "a pod the deployment manager makes runs only its own process's images": [
        "object.spec.initContainers.all(c, c.image in variables.images[variables.process])"
    ],
    "a pod the deployment manager makes runs exactly its own process's commands and arguments": [
        "variables.commands[variables.process]",
        "c.command",
        "c.args",
    ],
    "a pod the deployment manager makes mounts only memory, its own process's Secrets and projected token, or its claims read-only": [
        "has(v.emptyDir)",
        "v.secret.secretName in variables.secrets[variables.process]",
        "v.persistentVolumeClaim.claimName in variables.claims[variables.process]",
        "v.persistentVolumeClaim.readOnly == true",
        "size(v.projected.sources) == 2",
        "size(v.projected.sources[1].downwardAPI.items) == 1",
        "in variables.projected[variables.process]",
    ],
    "a pod the deployment manager makes reads no Secret through its environment or a pull secret": [
        "!has(object.spec.imagePullSecrets)",
        "!has(c.envFrom)",
        "!has(e.valueFrom)",
    ],
    "a pod the deployment manager makes is one of its processes, in this workspace": [
        '["app.kubernetes.io/managed-by"] == "mandate-deploy-manager"',
        '["mandate.dev/process"] in variables.egress',
        '["mandate.dev/workspace"] == "ws1"',
    ],
    "a pod the deployment manager makes carries exactly its process's egress labels": [
        'startsWith("mandate.dev/egress-")',
        'variables.egress[object.metadata.labels["mandate.dev/process"]].all(',
    ],
}


def policy_variables(policy: dict) -> dict:
    """The policy's list and map literals, which are JSON as well as CEL; `process`, the pod's own
    process label, and `account`, the account a request is about, are the variables that read the
    request."""
    return {
        v["name"]: json.loads(v["expression"])
        for v in policy["spec"].get("variables", [])
        if v["name"] not in ("process", "account")
    }


def refusals(policy: dict, pod: dict, first: bool = False) -> list[str]:
    """The policy's validations that refuse `pod`, its CEL evaluated."""
    return cel_refusals(policy, pod, first=first)


def as_started(pod: dict) -> dict:
    """A template as the manager creates it (crates/mandate-deploy-manager kubernetes.rs): its
    labels added, and the default account named, as the API server's ServiceAccount admission names
    it before any policy runs; a token exchange's account its connection's (DEC-2060), which its
    environment names beside the connection (DEC-1177)."""
    pod = copy.deepcopy(pod)
    pod["metadata"]["labels"].update(ADDED_LABELS)
    exchange = pod["metadata"]["labels"]["mandate.dev/process"] == EXCHANGE
    pod["spec"]["serviceAccountName"] = CONNECTION_ACCOUNT if exchange else "default"
    process_code = pod["metadata"]["labels"]["mandate.dev/process"].replace("-", "_")
    for c in pod["spec"]["containers"]:
        c["env"].append({"name": "MANDATE_PROCESS", "value": process_code})
        c["env"].append({"name": "MANDATE_SUBJECT", "value": CONNECTION if exchange else "subject_1"})
        if exchange:
            c["env"].append({"name": "MANDATE_SUBJECT_ACCOUNT", "value": CONNECTION_ACCOUNT})
    return pod


def attacks(pod: dict) -> list[tuple[str, dict]]:
    """What a compromised manager might create from a valid pod; the policy must refuse each."""

    def changed(name: str, change) -> tuple[str, dict]:
        bad = copy.deepcopy(pod)
        change(bad)
        return name, bad

    spec = lambda b: b["spec"]  # noqa: E731
    labels = lambda b: b["metadata"]["labels"]  # noqa: E731
    process_name = pod["metadata"]["labels"]["mandate.dev/process"]
    other = "runtime" if process_name == "executor" else "executor"
    foreign = sorted(
        {f"mandate.dev/egress-{t}" for t in process(other)["egress"]} - egress_labels(labels(pod))
    )
    first = lambda b: spec(b)["containers"][0]  # noqa: E731
    vault_token = {
        "defaultMode": 292,
        "sources": [
            {"serviceAccountToken": {"audience": "mandate-vault", "expirationSeconds": 600, "path": "token"}},
            {"downwardAPI": {"items": [{"path": "namespace", "fieldRef": {"fieldPath": "metadata.namespace"}}]}},
        ],
    }
    projected = lambda b: next(v for v in spec(b)["volumes"] if "projected" in v)["projected"]  # noqa: E731
    item = lambda b: projected(b)["sources"][1]["downwardAPI"]["items"][0]  # noqa: E731
    env = lambda b, name: next(e for e in first(b)["env"] if e["name"] == name)  # noqa: E731
    as_exchange = {
        "mandate.dev/process": EXCHANGE,
        **{f"mandate.dev/egress-{t}": "true" for t in process(EXCHANGE)["egress"]},
    }
    exchange_only = lambda: [  # noqa: E731
        changed(
            "another connection's form of account", lambda b: spec(b).update(serviceAccountName="exchange-1")
        ),
        changed("the default account", lambda b: spec(b).update(serviceAccountName="default")),
        changed("an executor's account", lambda b: spec(b).update(serviceAccountName="0123456789abcdef")),
        changed("an even-length x account", lambda b: spec(b).update(serviceAccountName="x0123456789abcde")),
        changed("an account longer than any id", lambda b: spec(b).update(serviceAccountName="x" + "ab" * 65)),
        changed("an account its environment does not name", lambda b: env(b, "MANDATE_SUBJECT_ACCOUNT").update(value="xab")),
        changed(
            "no account in its environment",
            lambda b: first(b)["env"].remove(env(b, "MANDATE_SUBJECT_ACCOUNT")),
        ),
        changed(
            "a second account in its environment",
            lambda b: first(b)["env"].append({"name": "MANDATE_SUBJECT_ACCOUNT", "value": "xab"}),
        ),
        changed(
            "a second connection in its environment",
            lambda b: first(b)["env"].append({"name": "MANDATE_SUBJECT", "value": "conn_other"}),
        ),
        changed("no namespace file", lambda b: projected(b)["sources"].pop(1)),
        changed("the namespace file first", lambda b: projected(b)["sources"].reverse()),
        changed("another downward field", lambda b: item(b)["fieldRef"].update(fieldPath="metadata.labels")),
        changed("a second downward item", lambda b: projected(b)["sources"][1]["downwardAPI"]["items"].append(
            {"path": "name", "fieldRef": {"fieldPath": "metadata.name"}}
        )),
        changed("a downward item's own mode", lambda b: item(b).update(mode=511)),
        changed("a wider projected mode", lambda b: projected(b).update(defaultMode=511)),
        changed(
            "the journal's Secret",
            lambda b: spec(b)["volumes"].append({"name": "c", "secret": {"secretName": "journal-ca"}}),
        ),
        changed(
            "a longer-lived token",
            lambda b: next(v for v in spec(b)["volumes"] if "projected" in v)["projected"]["sources"][0][
                "serviceAccountToken"
            ].update(expirationSeconds=86400),
        ),
        changed(
            "a third projected source",
            lambda b: projected(b)["sources"].append({"secret": {"name": "journal-ca"}}),
        ),
        changed(
            "a Secret in place of the namespace file",
            lambda b: projected(b)["sources"].__setitem__(1, {"secret": {"name": "journal-ca"}}),
        ),
        changed(
            "a token for another audience",
            lambda b: next(v for v in spec(b)["volumes"] if "projected" in v)["projected"]["sources"][0][
                "serviceAccountToken"
            ].update(audience="kubernetes"),
        ),
    ]
    others_only = lambda: [  # noqa: E731
        changed("an exchange's account", lambda b: spec(b).update(serviceAccountName="x0123456789abcdef")),
        changed(
            "the vault token", lambda b: spec(b)["volumes"].append({"name": "v", "projected": vault_token})
        ),
        changed(
            "the token exchange's labels",
            lambda b: (
                [labels(b).pop(k) for k in sorted(egress_labels(labels(b)))],
                labels(b).update(as_exchange),
                spec(b).update(serviceAccountName="x0123456789abcdef"),
            ),
        ),
    ]
    return [
        *(exchange_only() if process_name == EXCHANGE else others_only()),
        changed("an altered command", lambda b: first(b).update(command=["/bin/sh", "-c", "id"])),
        changed("altered arguments", lambda b: first(b).update(args=[*first(b).get("args", []), "--extra"])),
        changed(
            "an added container",
            lambda b: spec(b)["containers"].append({**copy.deepcopy(first(b)), "name": "y"}),
        ),
        changed(
            "another process's image",
            lambda b: first(b).update(image=PLACEHOLDER.format(process(other)["image"])),
        ),
        changed("a token", lambda b: spec(b).update(automountServiceAccountToken=True)),
        changed("the manager's account", lambda b: spec(b).update(serviceAccountName=MANAGER)),
        changed("the host network", lambda b: spec(b).update(hostNetwork=True)),
        changed("root, for the pod", lambda b: spec(b)["securityContext"].update(runAsUser=0)),
        changed("root allowed", lambda b: spec(b)["securityContext"].update(runAsNonRoot=False)),
        changed("root, for a container", lambda b: first(b)["securityContext"].update(runAsUser=0)),
        changed(
            "root allowed, for an init container",
            lambda b: spec(b)["initContainers"][0]["securityContext"].update(runAsNonRoot=False),
        ),
        changed(
            "an added capability",
            lambda b: first(b)["securityContext"]["capabilities"].update(add=["NET_ADMIN"]),
        ),
        changed("a host alias", lambda b: spec(b).update(hostAliases=[{"ip": "10.0.0.1", "hostnames": ["vault"]}])),
        changed("a chosen node", lambda b: spec(b).update(nodeName="node-1")),
        changed("the host's PIDs", lambda b: spec(b).update(hostPID=True)),
        changed("a host path", lambda b: spec(b)["volumes"].append({"name": "h", "hostPath": {"path": "/"}})),
        changed("privileged", lambda b: spec(b)["containers"][0]["securityContext"].update(privileged=True)),
        changed(
            "escalation",
            lambda b: spec(b)["initContainers"][0]["securityContext"].update(allowPrivilegeEscalation=True),
        ),
        changed(
            "an ephemeral container",
            lambda b: spec(b).update(ephemeralContainers=[{"name": "x", "image": "x"}]),
        ),
        changed("another image", lambda b: spec(b)["containers"][0].update(image="registry.example/shell:1")),
        changed(
            "another role's secret",
            lambda b: spec(b)["volumes"].append(
                {"name": "s", "secret": {"secretName": "journal-role-mandate-control"}}
            ),
        ),
        changed(
            "the artifact store read-write",
            lambda b: spec(b)["volumes"].append({"name": "w", "persistentVolumeClaim": {"claimName": "artifacts"}}),
        ),
        changed(
            "another claim",
            lambda b: spec(b)["volumes"].append(
                {"name": "o", "persistentVolumeClaim": {"claimName": "journal-1", "readOnly": True}}
            ),
        ),
        changed(
            "a projected token",
            lambda b: spec(b)["volumes"].append(
                {"name": "p", "projected": {"sources": [{"serviceAccountToken": {"path": "t"}}]}}
            ),
        ),
        changed(
            "a secret by env",
            lambda b: spec(b)["containers"][0]["env"].append(
                {"name": "X", "valueFrom": {"secretKeyRef": {"name": "journal-ca", "key": "ca.crt"}}}
            ),
        ),
        changed(
            "a secret by envFrom",
            lambda b: spec(b)["containers"][0].update(envFrom=[{"secretRef": {"name": "s"}}]),
        ),
        changed("a pull secret", lambda b: spec(b).update(imagePullSecrets=[{"name": "s"}])),
        changed("not the manager's", lambda b: labels(b).pop("app.kubernetes.io/managed-by")),
        changed("another process", lambda b: labels(b).update({"mandate.dev/process": "scheduler"})),
        changed("another workspace", lambda b: labels(b).update({"mandate.dev/workspace": "ws2"})),
        changed(
            "another process's egress",
            lambda b: labels(b).update({k: "true" for k in foreign or ["mandate.dev/egress-time"]}),
        ),
        changed("less egress", lambda b: labels(b).pop(sorted(egress_labels(labels(b)))[0])),
    ]


def admission_problems(docs: list[dict]) -> Iterator[str]:
    """The manager's `create pods` is bounded to pods shaped like its templates (DEC-1172 item 4):
    one ValidatingAdmissionPolicy and its binding exactly when the manager renders, failing closed,
    matching core pods' create and update in the namespace and only requests by the manager's own
    ServiceAccount, denying, with every bound in BOUNDS; its images are its templates', its Secrets
    are exactly the started processes' roles and the journal's CA, its claims only the artifact store
    the runtime reads (DEC-1016), and its egress labels are each
    process's from processes.json. Each template as started passes it, and each attack fails it."""
    manager = any(is_manager(w) for w in workloads(docs))
    found = [
        d for d in docs if d["kind"] in (POLICY, POLICY_BINDING) and d["metadata"]["name"].endswith(PODS_POLICY)
    ]
    if [d["kind"] for d in found] != ([POLICY, POLICY_BINDING] if manager else []):
        yield f"admission objects {[d['kind'] for d in found]} with the manager {'on' if manager else 'off'}"
        return
    if not manager:
        return
    policy, binding = found
    spec = policy["spec"]
    if spec.get("failurePolicy") != "Fail":
        yield f"the policy's failurePolicy is {spec.get('failurePolicy')}"
    if spec.get("matchConstraints") != {"namespaceSelector": NAMESPACE_SELECTOR, "resourceRules": POD_RULES}:
        yield f"the policy matches {spec.get('matchConstraints')}"
    conditions = [c["expression"] for c in spec.get("matchConditions", [])]
    if conditions != [f"request.userInfo.username == '{MANAGER_USER}'"]:
        yield f"the policy's match conditions are {conditions}"
    if binding["spec"] != {
        "policyName": policy["metadata"]["name"],
        "validationActions": ["Deny"],
        "matchResources": {"namespaceSelector": NAMESPACE_SELECTOR},
    }:
        yield f"the binding is {binding['spec']}"
    if not policy["metadata"]["name"].startswith("ws-ws1-cell-"):
        yield f"the policy's name {policy['metadata']['name']} carries no release prefix"
    started = managed_pods(docs)
    names = {w["metadata"]["name"] for w in started}
    claimed = any(d["kind"] == "PersistentVolumeClaim" and d["metadata"]["name"] == "artifacts" for d in docs)
    templates = {w["metadata"]["name"]: w["spec"]["template"] for w in started}
    role = lambda n: process(n)["db_role"]  # noqa: E731
    want = {
        "images": {n: sorted({c["image"] for c in all_containers(t)}) for n, t in templates.items()},
        "commands": {n: launches(t) for n, t in templates.items()},
        "secrets": {
            n: sorted({"journal-ca", "journal-role-" + role(n).replace("_", "-")}) if role(n) else []
            for n in names
        },
        "claims": {n: ["artifacts"] if n == "runtime" and claimed else [] for n in names},
        "projected": {
            n: ["mandate-vault|600|token|292|namespace=metadata.namespace"]
            if n == EXCHANGE
            else []
            for n in names
        },
        "egress": {n: [f"mandate.dev/egress-{t}" for t in process(n)["egress"]] for n in names},
    }
    try:
        got = policy_variables(policy)
    except (json.JSONDecodeError, KeyError) as e:
        yield f"the policy's lists do not read: {e}"
        return
    for key, value in want.items():
        if got.get(key) != value:
            yield f"the policy's {key} is {got.get(key)}, want {value}"
    if set(got) != set(want):
        yield f"the policy's variables are {sorted(got)}, want {sorted(want)}"
    reads = [v for v in spec.get("variables", []) if v["name"] == "process"]
    if [v["name"] for v in spec.get("variables", [])][:1] != ["process"] or [
        v["expression"] for v in reads
    ] != [PROCESS_VARIABLE]:
        yield f"the policy does not read the pod's process first: {reads}"
    rules = {v["message"]: v["expression"] for v in spec.get("validations", [])}
    for message, words in BOUNDS.items():
        if message not in rules:
            yield f"the policy does not hold: {message}"
        elif not all(w in rules[message] for w in words):
            yield f"the policy's rule `{message}` does not name {words}: {rules[message]}"
    for w in started:
        pod = as_started(w["spec"]["template"])
        if refusals(policy, pod):
            yield (
                f"the policy refuses the {w['metadata']['name']} template: {refusals(policy, pod)}"
            )
        for attack, bad in attacks(pod):
            if not refusals(policy, bad, first=True):
                yield f"the policy admits a {w['metadata']['name']} pod with {attack}"


def deploy_manager_admission(docs: list[dict]) -> list[str]:
    return list(admission_problems(docs))


@functools.cache
def admission_holds() -> bool:
    """The unseeded chart's pod admission, checked once for every seeded bug."""
    return deploy_manager_admission(render_all()) == []


@functools.cache
def accounts_hold() -> bool:
    """The unseeded chart's account admission, checked once for every seeded bug."""
    return deploy_manager_accounts(render_all()) == []


# Each bound the ServiceAccounts policy must hold (DEC-1177), as BOUNDS for pods: the words its CEL
# must name; what it admits is its CEL, evaluated.
ACCOUNT_BOUNDS = {
    "an account the deployment manager makes or deletes is named for a connection's token exchange": [
        "variables.account.metadata.name.matches('^x([0-9a-f]{2}){1,64}$')"
    ],
    "an account the deployment manager makes or deletes mounts no token and holds no Secret": [
        "variables.account.automountServiceAccountToken == false",
        "!has(variables.account.secrets)",
        "!has(variables.account.imagePullSecrets)",
    ],
    "an account the deployment manager makes or deletes is a token exchange's, in this workspace": [
        '["app.kubernetes.io/managed-by"] == "mandate-deploy-manager"',
        '["mandate.dev/workspace"] == "ws1"',
        '["mandate.dev/process"] == "token-exchange"',
    ],
}
ACCOUNT_VARIABLE = 'request.operation == "DELETE" ? oldObject : object'


def account(name: str = CONNECTION_ACCOUNT, process_name: str = EXCHANGE) -> dict:
    """A ServiceAccount as the manager creates it for a connection's token exchange (DEC-1177)."""
    return {
        "apiVersion": "v1",
        "kind": "ServiceAccount",
        "metadata": {
            "name": name,
            "labels": {
                "app.kubernetes.io/managed-by": "mandate-deploy-manager",
                "mandate.dev/workspace": "ws1",
                "mandate.dev/process": process_name,
            },
        },
        "automountServiceAccountToken": False,
    }


def account_refusals(policy: dict, sa: dict, operation: str = "CREATE", first: bool = False) -> list[str]:
    """The policy's validations that refuse the manager's `operation` on `sa`, its CEL evaluated; a
    delete reads the account as it stands."""
    if operation == "DELETE":
        return cel_refusals(policy, None, sa, operation, first)
    return cel_refusals(policy, sa, None, operation, first)


def account_attacks() -> list[tuple[str, dict]]:
    def changed(name: str, change) -> tuple[str, dict]:
        bad = account()
        change(bad)
        return name, bad

    meta = lambda b: b["metadata"]  # noqa: E731
    labels = lambda b: b["metadata"]["labels"]  # noqa: E731
    return [
        changed("the manager's own name", lambda b: meta(b).update(name=MANAGER)),
        changed("the default account", lambda b: meta(b).update(name="default")),
        changed("an upper-case name", lambda b: meta(b).update(name=CONNECTION_ACCOUNT.upper())),
        changed("a bare x", lambda b: meta(b).update(name="x")),
        changed("an odd count of hex digits", lambda b: meta(b).update(name=CONNECTION_ACCOUNT[:-1])),
        changed("a name longer than any id", lambda b: meta(b).update(name="x" + "ab" * 65)),
        changed("an executor's name", lambda b: meta(b).update(name=CONNECTION_ACCOUNT[1:])),
        changed("a mounted token", lambda b: b.update(automountServiceAccountToken=True)),
        changed("no automount setting", lambda b: b.pop("automountServiceAccountToken")),
        changed("a Secret", lambda b: b.update(secrets=[{"name": "journal-ca"}])),
        changed("a pull secret", lambda b: b.update(imagePullSecrets=[{"name": "s"}])),
        changed("not the manager's", lambda b: labels(b).pop("app.kubernetes.io/managed-by")),
        changed("another workspace", lambda b: labels(b).update({"mandate.dev/workspace": "ws2"})),
        changed("an executor's label", lambda b: labels(b).update({"mandate.dev/process": "executor"})),
        changed("a runtime's label", lambda b: labels(b).update({"mandate.dev/process": "runtime"})),
    ]


def deploy_manager_accounts(docs: list[dict]) -> list[str]:
    """The manager's `create` and `delete serviceaccounts` are bounded as its pods are (DEC-1177):
    one policy and its binding exactly when the manager renders, failing closed, matching core
    ServiceAccounts' create, update and delete in the namespace and only the manager's own
    requests, denying, with every bound in ACCOUNT_BOUNDS. A token exchange's account as the
    manager makes it passes on create, update and delete; each attack fails on each, so the manager
    can neither make nor delete the default account, its own, or another workload's."""
    manager = any(is_manager(w) for w in workloads(docs))
    found = [
        d for d in docs if d["kind"] in (POLICY, POLICY_BINDING) and d["metadata"]["name"].endswith(ACCOUNTS_POLICY)
    ]
    if [d["kind"] for d in found] != ([POLICY, POLICY_BINDING] if manager else []):
        return [f"account admission objects {[d['kind'] for d in found]} with the manager {'on' if manager else 'off'}"]
    if not manager:
        return []
    problems = []
    policy, binding = found
    spec = policy["spec"]
    if spec.get("failurePolicy") != "Fail":
        problems.append(f"the account policy's failurePolicy is {spec.get('failurePolicy')}")
    if spec.get("matchConstraints") != {"namespaceSelector": NAMESPACE_SELECTOR, "resourceRules": ACCOUNT_RULES}:
        problems.append(f"the account policy matches {spec.get('matchConstraints')}")
    conditions = [c["expression"] for c in spec.get("matchConditions", [])]
    if conditions != [f"request.userInfo.username == '{MANAGER_USER}'"]:
        problems.append(f"the account policy's match conditions are {conditions}")
    if [(v["name"], v["expression"]) for v in spec.get("variables", [])] != [("account", ACCOUNT_VARIABLE)]:
        problems.append(f"the account policy does not read a delete's account as it stands: {spec.get('variables')}")
    if binding["spec"] != {
        "policyName": policy["metadata"]["name"],
        "validationActions": ["Deny"],
        "matchResources": {"namespaceSelector": NAMESPACE_SELECTOR},
    }:
        problems.append(f"the account binding is {binding['spec']}")
    rules = {v["message"]: v["expression"] for v in spec.get("validations", [])}
    for message, words in ACCOUNT_BOUNDS.items():
        if message not in rules:
            problems.append(f"the account policy does not hold: {message}")
        elif not all(w in rules[message] for w in words):
            problems.append(f"the account policy's rule `{message}` does not name {words}: {rules[message]}")
    for operation in ("CREATE", "UPDATE", "DELETE"):
        if account_refusals(policy, account(), operation):
            problems.append(f"the account policy refuses the exchange's {operation}: {account_refusals(policy, account(), operation)}")
        for attack, bad in account_attacks():
            if not account_refusals(policy, bad, operation, first=True):
                problems.append(f"the account policy admits {operation} of an account with {attack}")
    return problems


def test_deploy_manager_accounts():
    docs = render_all()
    assert deploy_manager_accounts(docs) == []
    without = render_all("--set", f"images.{MANAGER}=")
    assert deploy_manager_accounts(without) == []


ACCOUNTS_SEEDED = [
    lambda d: [x for x in d if not (x["kind"] == POLICY and x["metadata"]["name"].endswith(ACCOUNTS_POLICY))],
    lambda d: mutate(
        d, policy_of(d, ACCOUNTS_POLICY), POLICY, lambda p: p["spec"].update(failurePolicy="Ignore")
    ),
    lambda d: mutate(d, policy_of(d, ACCOUNTS_POLICY), POLICY, lambda p: p["spec"].pop("matchConditions")),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        drop_rule("an account the deployment manager makes or deletes is named for a connection's token exchange"),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        rewrite_rule(
            "an account the deployment manager makes or deletes is named for a connection's token exchange",
            "matches('^x([0-9a-f]{2}){1,64}$')",
            "matches('^x?[0-9a-f]+$')",
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        drop_rule("an account the deployment manager makes or deletes mounts no token and holds no Secret"),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        drop_rule("an account the deployment manager makes or deletes is a token exchange's, in this workspace"),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY_BINDING,
        lambda b: b["spec"].update(validationActions=["Warn"]),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        loosen("an account the deployment manager makes or deletes is named for a connection's token exchange"),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        loosen("an account the deployment manager makes or deletes mounts no token and holds no Secret"),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        lambda p: p["spec"]["variables"][0].update(expression="object"),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        lambda p: p["spec"]["matchConstraints"]["resourceRules"][0]["operations"].remove("DELETE"),
    ),
]


@pytest.mark.parametrize("seed", ACCOUNTS_SEEDED, ids=[f"accounts-{i}" for i in range(len(ACCOUNTS_SEEDED))])
def test_each_account_seeded_bug_is_caught(seed):
    docs = render_all()
    assert accounts_hold(), "the unseeded chart already fails"
    assert deploy_manager_accounts(seed(docs)), "deploy_manager_accounts missed its seeded bug"


def test_the_manager_role_seeded_bugs_are_caught():
    docs = render_all()
    assert deploy_manager_contained(docs) == []
    for change in (
        lambda r: r["rules"][1]["verbs"].append("update"),
        lambda r: r["rules"][1]["resources"].append("secrets"),
        lambda r: r["rules"].pop(1),
    ):
        assert deploy_manager_contained(mutate(docs, MANAGER, "Role", change)), change


def cel_shape(value, at: str = "$"):
    """The CEL type of a JSON literal, or `TypeError` naming where it mixes two: the API server
    refuses a policy whose map or list literal holds values of different types (#2077's cell run),
    so a mirror of the rules over a mixed literal could pass while the cell cannot install it. An
    empty list takes the type of the lists beside it."""
    def merge(shapes, where):
        known = {s for s in shapes if s is not None}
        if len(known) > 1:
            raise TypeError(f"{where} mixes {sorted(map(str, known))}")
        return next(iter(known), None)

    if isinstance(value, dict):
        return ("map", merge([cel_shape(v, f"{at}.{k}") for k, v in value.items()], at))
    if isinstance(value, list):
        return ("list", merge([cel_shape(v, f"{at}[{i}]") for i, v in enumerate(value)], at)) if value else None
    return type(value).__name__


def test_every_admission_literal_has_one_type_so_cel_compiles_it():
    for docs in (render_all(), render_all("--set", f"images.{EXCHANGE}=")):
        for policy in (d for d in docs if d["kind"] == POLICY):
            for name, value in policy_variables(policy).items():
                cel_shape(value, name)
    with pytest.raises(TypeError, match="commands"):
        cel_shape({"executor": [{"name": "executor", "args": ["run"]}]}, "commands")
    with pytest.raises(TypeError, match="projected"):
        cel_shape({"token-exchange": [{"audience": "mandate-vault", "expirationSeconds": 600}]}, "projected")


def test_deploy_manager_admission():
    docs = render_all()
    assert deploy_manager_admission(docs) == []
    assert {d["kind"] for d in docs} >= {POLICY, POLICY_BINDING}
    without = render_all("--set", f"images.{MANAGER}=")
    assert deploy_manager_admission(without) == [] and not [
        d for d in without if d["kind"] in (POLICY, POLICY_BINDING)
    ]
    only_runtime = render_all("--set", "images.executor=", "--set", f"images.{EXCHANGE}=")
    assert deploy_manager_admission(only_runtime) == []
    policy = next(d for d in only_runtime if d["kind"] == POLICY)
    assert policy_variables(policy)["secrets"] == {"runtime": ["journal-ca", "journal-role-mandate-runtime"]}


def test_the_token_exchange_is_a_managed_one_shot_pod():
    """DEC-2627, DEC-1175: with its image named, the token exchange is a template the manager
    starts, never a standing workload; no Secret, only its projected vault token (DEC-2060)."""
    docs = render_all()
    assert not [d for d in docs if d["kind"] in ("Deployment", "Job") and d["metadata"]["name"] == EXCHANGE]
    pod = next(w for w in managed_pods(docs) if w["metadata"]["name"] == EXCHANGE)["spec"]["template"]
    volumes = pod["spec"]["volumes"]
    assert not [v for v in volumes if "secret" in v]
    assert [v["projected"] for v in volumes if "projected" in v] == [
        {
            "defaultMode": 292,
            "sources": [
                {
                    "serviceAccountToken": {
                        "audience": "mandate-vault",
                        "expirationSeconds": 600,
                        "path": "token",
                    }
                },
                {"downwardAPI": {"items": [{"path": "namespace", "fieldRef": {"fieldPath": "metadata.namespace"}}]}},
            ],
        }
    ]
    mounts = [m for c in pod["spec"]["containers"] for m in c["volumeMounts"] if m["name"] == "vault-token"]
    assert mounts == [{"name": "vault-token", "mountPath": "/run/mandate/vault-token", "readOnly": True}]
    assert pod["spec"]["automountServiceAccountToken"] is False
    assert pod["metadata"]["labels"]["mandate.dev/process"] == EXCHANGE
    assert pod["metadata"]["labels"]["mandate.dev/workspace"] == "ws1"
STUB = "mandate.local/stub-issuer:0123456789ab"
STUB_BIND = "127.0.0.1:8090"


def render_with_stub(*extra: str) -> list[dict]:
    return render_all(
        "--set", f"images.stub-issuer={STUB}", "--set", "api.subjectLinks=subject-links", *extra
    )


def stub_sidecar(docs: list[dict]) -> list[str]:
    """The local stub issuer (DEC-2100, DEC-1025), when rendered, is a native sidecar of the API pod
    on its loopback alone, in a dev or ci tier, from a local build; a readiness wait follows it, so
    the server starts only once the stub answers, and the server's key set is the stub's."""
    problems = []
    for w in [w for w in workloads(docs) if w["metadata"]["name"] == "api"]:
        inits = pod_spec(w).get("initContainers", [])
        names = [c["name"] for c in inits]
        if "stub-issuer" not in names:
            continue
        at = names.index("stub-issuer")
        stub = inits[at]
        if stub.get("restartPolicy") != "Always":
            problems.append("the stub issuer is not a sidecar, so it would block the pod's start")
        if stub.get("args") != ["--bind", STUB_BIND]:
            problems.append(f"the stub issuer binds beyond loopback: {stub.get('args')}")
        if not stub.get("image", "").startswith("mandate.local/"):
            problems.append(f"the stub issuer is not a local build: {stub.get('image')}")
        tier = {e["name"]: e.get("value") for e in stub.get("env", [])}.get("MANDATE_DEPLOYMENT_TIER")
        if tier not in ("dev", "ci"):
            problems.append(f"the stub issuer runs in tier {tier!r}")
        ready = names[at + 1] if at + 1 < len(names) else None
        script = " ".join(inits[at + 1].get("command", [])) if ready else ""
        if ready != "stub-issuer-ready" or "/dev/tcp/127.0.0.1/8090" not in script or "exit 1" not in script:
            problems.append("the server may start before the stub issuer answers")
        api = next(c for c in pod_spec(w)["containers"] if c["name"] == "api")
        env = {e["name"]: e.get("value") for e in api.get("env", [])}
        if (
            env.get("MANDATE_API_JWKS_URL") != f"http://{STUB_BIND}/jwks.json"
            or env.get("MANDATE_API_ISSUER") != f"http://{STUB_BIND}"
        ):
            problems.append(f"the server does not trust the stub it runs beside: {env}")
        if env.get("MANDATE_DEPLOYMENT_TIER") not in ("dev", "ci"):
            problems.append("the server beside the stub is not in a dev or ci tier")
    return problems


def test_stub_sidecar():
    docs = render_with_stub()
    assert stub_sidecar(docs) == [] and api_contained(docs) == []
    assert hardened(docs) == [] and gated(docs) == [] and secrets_as_files(docs) == []
    names = [c["name"] for c in pod_spec(owner_workload(docs))["initContainers"]]
    assert names.index("stub-issuer") < names.index("stub-issuer-ready")
    plain = render_all()
    assert "stub-issuer" not in [c["name"] for c in pod_spec(owner_workload(plain))["initContainers"]]
    for refused in (
        ["-f", str(CELL / "values" / "k3s.yaml"), "--set", f"images.stub-issuer={STUB}"],
        ["-f", str(CELL / "values" / "managed.yaml"), "--set", f"images.stub-issuer={STUB}"],
        [
            "-f",
            str(CELL / "values" / "k3s.yaml"),
            "-f",
            str(CELL.parent / "hosted" / "values" / "cell1.yaml"),
            "--set",
            f"images.stub-issuer={STUB}",
        ],
        [
            "-f",
            str(CELL / "values" / "k3d.yaml"),
            *every_image(),
            "--set",
            f"images.stub-issuer={STUB}",
            "--set",
            "deploymentTier=staging",
        ],
        [
            "-f",
            str(CELL / "values" / "k3d.yaml"),
            *every_image(),
            "--set",
            f"images.stub-issuer={STUB}",
            "--set",
            "deploymentTier=production",
        ],
        [
            "-f",
            str(CELL / "values" / "k3d.yaml"),
            *every_image(),
            "--set",
            "images.stub-issuer=registry.example/stub@sha256:" + "0" * 64,
        ],
    ):
        assert helm(CHART, *refused).returncode != 0, refused


def owner_workload(docs: list[dict]) -> dict:
    return next(w for w in workloads(docs) if w["metadata"]["name"] == "api")


def without_ingress(docs: list[dict]) -> list[dict]:
    """`docs` without the tunnel: its Deployment and policy, and its container and token volume in
    the API's pod (DEC-2056)."""
    docs = copy.deepcopy([d for d in docs if d["metadata"]["name"] not in ("cloudflared", "egress-cloudflared")])
    for w in workloads(docs):
        spec = pod_spec(w)
        spec["containers"] = [c for c in spec["containers"] if c["name"] != "cloudflared"]
        spec["volumes"] = [v for v in spec.get("volumes", []) if v["name"] != "tunnel"]
    return docs


def test_portable_across_k3d_k3s_and_managed():
    renders = [render_all("-f", str(CELL / "values" / v), "--set", "localImages=true") for v in VALUES_FILES]
    for docs in renders:
        assert portable(docs) == []
    assert renders[1] == renders[2], "k3s and a managed cluster render the same chart from the same values"
    assert without_ingress(renders[0]) == without_ingress(renders[1]), "k3d differs only by the tunnel"


def ingress_only_by_tunnel(docs: list[dict]) -> list[str]:
    """The cell's one ingress is cloudflared dialing out (DEC-967 item 6): no Service, NodePort,
    LoadBalancer or Ingress object; the tunnel token a mounted file, never a value or an
    environment variable; and its egress DNS and the tunnel edge on port 7844 only."""
    problems = [f"{d['kind']} {d['metadata']['name']}" for d in docs if d["kind"] in ("Service", "Ingress")]
    tunnels = [d for d in docs if d["kind"] == "Deployment" and d["metadata"]["name"] == "cloudflared"]
    for t in tunnels:
        c = pod_spec(t)["containers"][0]
        if "--token" in c.get("args", []) or any("TUNNEL_TOKEN" in e["name"] for e in c.get("env", [])):
            problems.append("the tunnel token is not a file")
        if "/run/secrets/tunnel/token" not in c.get("args", []):
            problems.append("the tunnel does not read its token file")
        policy = policies(docs).get("egress-cloudflared")
        if policy is None:
            problems.append("no egress policy for the tunnel")
            continue
        got = set().union(*(rule_destinations(r, {}) for r in policy["spec"].get("egress") or []))
        if any(d not in DNS and d[2] != 7844 for d in got) or policy["spec"].get("ingress"):
            problems.append(f"the tunnel's egress is {sorted(got)}")
    return problems


def test_ingress_only_by_tunnel():
    tunnel = render("-f", str(CELL / "values" / "k3s.yaml"))
    assert [d["metadata"]["name"] for d in tunnel if d["kind"] == "Deployment"] == ["cloudflared"]
    assert ingress_only_by_tunnel(tunnel) == []
    assert hardened(tunnel) == [] and gated(tunnel) == []
    assert not [d for d in render_all() if d["metadata"]["name"] == "cloudflared"], "off in the local cell"
    for bad in (
        "ingress.cloudflared.tokenFrom=",
        "ingress.cloudflared.image=cloudflare/cloudflared:latest",
    ):
        assert helm(CHART, "-f", str(CELL / "values" / "k3s.yaml"), "--set", bad).returncode != 0, bad
    leaky = mutate(
        tunnel,
        "cloudflared",
        "Deployment",
        lambda w: first_container(w)["env"].append({"name": "TUNNEL_TOKEN", "value": "x"}),
    )
    assert ingress_only_by_tunnel(leaky)
    exposed = [
        *tunnel,
        {"kind": "Service", "apiVersion": "v1", "metadata": {"name": "api"}, "spec": {"type": "NodePort"}},
    ]
    assert ingress_only_by_tunnel(exposed)
    wide = mutate(
        tunnel,
        "egress-cloudflared",
        "NetworkPolicy",
        lambda p: p["spec"]["egress"].append(
            {"to": [{"ipBlock": {"cidr": "192.0.2.0/24"}}], "ports": [{"port": 443}]}
        ),
    )
    assert ingress_only_by_tunnel(wide)


TUNNEL_TOKEN = "/run/secrets/tunnel/token"


def tunnel_route(docs: list[dict]) -> list[str]:
    """The tunnel reaches the API's loopback listener, the origin lane IAC routes the API's host to
    (`http://127.0.0.1:8080`), by running in the API's pod: with the API, cloudflared is a second
    container there and no other pod runs one, so every connector of the tunnel can reach the API;
    without it, cloudflared runs alone. Only cloudflared reads the tunnel token, it mounts nothing of
    the API's, and the tunnel's egress policy selects the pod it runs in."""
    problems = []
    if "egress-cloudflared" not in policies(docs):
        return problems
    runners = [
        (w, c) for w in workloads(docs) for c in pod_spec(w)["containers"] if c["name"] == "cloudflared"
    ]
    api = [w for w in workloads(docs) if w["metadata"]["name"] == "api"]
    want = api[0]["metadata"]["name"] if api else "cloudflared"
    if [w["metadata"]["name"] for w, _ in runners] != [want]:
        problems.append(f"cloudflared runs in {[w['metadata']['name'] for w, _ in runners]}, want [{want!r}]")
        return problems
    pod, tunnel = runners[0]
    selector = policies(docs)["egress-cloudflared"]["spec"]["podSelector"]["matchLabels"]
    if any(pod["spec"]["template"]["metadata"]["labels"].get(k) != v for k, v in selector.items()):
        problems.append(f"the tunnel's egress policy selects {selector}, not its pod")
    token_volume = next(
        (
            v["name"]
            for v in pod_spec(pod).get("volumes", [])
            if any(i.get("path") == "token" for i in v.get("secret", {}).get("items", []))
        ),
        None,
    )
    for c in [*pod_spec(pod).get("initContainers", []), *pod_spec(pod)["containers"]]:
        names = {m["name"] for m in c.get("volumeMounts", [])}
        if c is tunnel:
            if token_volume not in names or TUNNEL_TOKEN not in c.get("args", []):
                problems.append("cloudflared does not read the tunnel token's file")
            if names - {token_volume, "tmp"}:
                problems.append(f"cloudflared mounts {sorted(names - {token_volume, 'tmp'})}")
        elif token_volume in names:
            problems.append(f"{c['name']} mounts the tunnel token")
    return problems


def tunnelled(*extra: str) -> list[dict]:
    return render_all("-f", str(CELL / "values" / "managed.yaml"), "--set", "localImages=true", *extra)


def test_the_tunnel_runs_beside_the_api():
    docs = tunnelled()
    assert tunnel_route(docs) == []
    api = owner_workload(docs)
    assert [c["name"] for c in pod_spec(api)["containers"]] == ["api", "cloudflared"]
    assert api_contained(docs) == [], "the API still listens on its pod's loopback only"
    assert not [d for d in docs if d["kind"] in ("Service", "Ingress")]
    alone = render("-f", str(CELL / "values" / "k3s.yaml"))
    assert tunnel_route(alone) == []
    assert [d["metadata"]["name"] for d in alone if d["kind"] == "Deployment"] == ["cloudflared"]
    standalone = next(d for d in alone if d["kind"] == "Deployment" and d["metadata"]["name"] == "cloudflared")
    seeds = [
        [*docs, standalone],
        mutate(docs, "api", "Deployment", lambda w: pod_spec(w)["containers"].pop()),
        mutate(
            docs,
            "egress-cloudflared",
            "NetworkPolicy",
            lambda p: p["spec"]["podSelector"]["matchLabels"].update({"mandate.dev/process": "cloudflared"}),
        ),
        mutate(
            docs,
            "api",
            "Deployment",
            lambda w: first_container(w)["volumeMounts"].append(
                {"name": "tunnel", "mountPath": "/run/secrets/tunnel", "readOnly": True}
            ),
        ),
        mutate(
            docs,
            "api",
            "Deployment",
            lambda w: pod_spec(w)["containers"][1]["volumeMounts"].append(
                {"name": "credentials", "mountPath": "/run/credentials", "readOnly": True}
            ),
        ),
    ]
    for seeded in seeds:
        assert tunnel_route(seeded), seeded


ALPACA_SET_VALUES = {
    "MANDATE_ALPACA_CLIENT_ID": "PKTESTCLIENT01",
    "MANDATE_ALPACA_REDIRECT_URI": "https://api.example/v1/oauth/alpaca/callback",
}
ALPACA_PROCESSES = {"api", "token-exchange"}
ALPACA_SET = (
    "--set",
    f"alpacaOAuth.clientId={ALPACA_SET_VALUES['MANDATE_ALPACA_CLIENT_ID']}",
    "--set",
    f"alpacaOAuth.redirectUri={ALPACA_SET_VALUES['MANDATE_ALPACA_REDIRECT_URI']}",
)


def alpaca_oauth_env(docs: list[dict], expected: dict[str, str]) -> list[str]:
    """DEC-2636: the Alpaca OAuth client id and redirect URI reach only the API's and the token
    exchange's own containers, each exactly as `expected` names them (so both send the identical
    URI), neither when `expected` is empty, and no other container at all."""
    problems = []
    for w in workloads(docs):
        process = w["metadata"].get("labels", {}).get("mandate.dev/process")
        for c in pod_spec(w)["containers"]:
            carried = {e["name"]: e.get("value") for e in c.get("env", []) if e["name"] in ALPACA_SET_VALUES}
            wanted = expected if process in ALPACA_PROCESSES and c["name"] == process else {}
            if carried != wanted:
                problems.append(f"{w['metadata']['name']}/{c['name']} carries {carried}")
    return problems


def test_the_alpaca_oauth_settings_reach_the_api_and_the_token_exchange_only():
    assert alpaca_oauth_env(render_all(), {}) == []
    docs = render_all(*ALPACA_SET)
    assert alpaca_oauth_env(docs, ALPACA_SET_VALUES) == []
    leaked = mutate(
        docs,
        "scheduler",
        "Deployment",
        lambda w: first_container(w).setdefault("env", []).append({"name": "MANDATE_ALPACA_CLIENT_ID", "value": "x"}),
    )
    assert alpaca_oauth_env(leaked, ALPACA_SET_VALUES), "a third process given the client id is caught"
    dropped = mutate(
        docs,
        "token-exchange",
        "Deployment",
        lambda w: first_container(w).update(
            env=[e for e in first_container(w)["env"] if e["name"] != "MANDATE_ALPACA_REDIRECT_URI"]
        ),
    )
    assert alpaca_oauth_env(dropped, ALPACA_SET_VALUES), "the exchange without the redirect URI is caught"
    differs = mutate(
        docs,
        "token-exchange",
        "Deployment",
        lambda w: next(e for e in first_container(w)["env"] if e["name"] == "MANDATE_ALPACA_REDIRECT_URI").update(
            value="https://other.example/v1/oauth/alpaca/callback"
        ),
    )
    assert alpaca_oauth_env(differs, ALPACA_SET_VALUES), "an exchange sending another URI is caught"
    for refused in (
        "alpacaOAuth.clientId=has space",
        "alpacaOAuth.redirectUri=http://evil.example/v1/oauth/alpaca/callback",
        "alpacaOAuth.redirectUri=https://api.example/elsewhere",
        "alpacaOAuth.redirectUri=https://api.example/v1/oauth/alpaca/callback?x=1",
        "alpacaOAuth.redirectUri=https://api.example/v1/oauth/alpaca/callback#f",
    ):
        assert helm(CHART, "--set", refused).returncode != 0, refused


def test_environment_is_paper_or_backtest():
    assert render("--set", "environment=backtest")
    for refused in ("live", "production", ""):
        assert helm(CHART, "--set", f"environment={refused}").returncode != 0, refused


def tiers(docs: list[dict]) -> set[str]:
    """MANDATE_DEPLOYMENT_TIER of every process container (each one that sets HOME, as every
    process container does): the start-up check reads it (DEC-971 item 2)."""
    found = set()
    for w in workloads(docs):
        for c in pod_spec(w)["containers"]:
            names = [e["name"] for e in c.get("env", [])]
            assert len(names) == len(set(names)), f"{w['metadata']['name']} sets an env var twice: {names}"
            env = {e["name"]: e.get("value") for e in c.get("env", [])}
            if "HOME" in env and c["name"] != "cloudflared":
                found.add(env.get("MANDATE_DEPLOYMENT_TIER", "unset"))
    return found


def test_deployment_tier_is_set_from_values():
    hosted = CELL.parent / "hosted" / "values" / "cell1.yaml"
    api = ["--set", f"images.api={PLACEHOLDER.format('api')}"]
    images = every_image()
    at = next(i for i in range(len(images)) if images[i : i + 2] == api)
    manager_workspace = ["--set", "api.workspace=01KDEM0000000000000000000W"]
    without_api = images[:at] + images[at + 2 :] + manager_workspace
    assert tiers(render(*without_api)) == {"production"}, "unset is production"
    assert tiers(render_all()) == {"ci"}, (
        "the local cell, its API server too, runs its own unsigned build as ci"
    )
    k3s = ("-f", str(CELL / "values" / "k3s.yaml"))
    assert tiers(render(*k3s, "-f", str(hosted), *without_api)) == {"dev"}, "cell #1 is dev (DEC-2001)"
    api_block = [
        "api.org=01KDEM0000000000000000000R",
        "api.workspace=01KDEM0000000000000000000W",
        "api.issuer=https://issuer.example",
        "api.audience=authenticated",
        "api.jwksUrl=https://issuer.example/jwks",
        "api.appOrigin=https://app.example",
    ]
    with_api = [arg for s in api_block for arg in ("--set", s)]
    assert tiers(render(*k3s, "-f", str(hosted), *images, *with_api)) == {"dev"}, (
        "cell #1's API server takes the one tier too"
    )
    for refused in ("live", "prod", ""):
        assert helm(CHART, "--set", f"deploymentTier={refused}").returncode != 0, refused


def test_images_are_pinned():
    for image in ("registry.example/x:latest", "registry.example/x", "mandate.local/mandate:latest"):
        assert helm(CHART, "--set", f"images.mandate={image}").returncode != 0, image
    local = "images.mandate=mandate.local/mandate:0123456789ab"
    assert render("-f", str(CELL / "values" / "k3d.yaml"), "--set", local)
    for values in ("k3s.yaml", "managed.yaml"):
        assert helm(CHART, "-f", str(CELL / "values" / values), "--set", local).returncode != 0, values
    assert helm(CHART, "--set", local).returncode != 0, "only the local cell's values allow a local build"


def test_external_targets_are_narrow():
    for cidr in ("0.0.0.0/0", "10.0.0.0/8", "192.0.2.0/23", "999.1.1.1/32", "192.0.2.1"):
        assert helm(CHART, "--set-json", f'egress.broker=["{cidr}"]').returncode != 0, cidr
    assert helm(CHART, "--set-json", 'egress.token-host=["192.0.2.0/24"]').returncode != 0
    assert render("--set-json", 'egress.broker=["192.0.2.0/24"]')


def test_values_take_no_unknown_key():
    assert helm(CHART, "--set", "password=x").returncode != 0
    assert helm(JOURNAL_CHART, *JOURNAL_ARGS).returncode == 0
    assert helm(JOURNAL_CHART, *JOURNAL_ARGS, "--set", "superuserPassword=x").returncode != 0


def test_a_stopping_journal_waits_briefly_for_its_pooled_clients():
    """The runtime's pool never disconnects by itself, so CloudNativePG's default smart shutdown
    (180 s) would hold every primary restart that long before the fast one (DEC-1013 item 6)."""
    cluster = next(d for d in render(*JOURNAL_ARGS, chart=JOURNAL_CHART) if d["kind"] == "Cluster")
    assert cluster["spec"]["smartShutdownTimeout"] == 10
    assert helm(JOURNAL_CHART, *JOURNAL_ARGS, "--set", "smartShutdownTimeout=600").returncode != 0
    assert helm(JOURNAL_CHART, *JOURNAL_ARGS, "--set", "smartShutdownTimeout=180").returncode == 0


def test_roles_match():
    assert roles_match(render(*JOURNAL_ARGS, chart=JOURNAL_CHART)) == []


def test_compose_matches():
    assert compose_matches(COMPOSE) == []


def mutate(docs: list[dict], name: str, kind: str, change) -> list[dict]:
    """`docs` with `change` made to one object; kind `Pod` names a pod template of the manager's
    ConfigMap, changed where it is rendered, in that ConfigMap's entry."""
    docs = copy.deepcopy(docs)
    if kind == "Pod":
        templates = next(d for d in docs if d["kind"] == "ConfigMap" and d["metadata"]["name"] == TEMPLATES)
        workload = managed_pod(name, json.loads(templates["data"][f"{name}.json"]))
        change(workload)
        templates["data"][f"{name}.json"] = json.dumps(workload["spec"]["template"])
        return docs
    target = next(d for d in docs if d["metadata"]["name"] == name and d["kind"] == kind)
    change(target)
    return docs


def first_container(w: dict) -> dict:
    return pod_spec(w)["containers"][0]


def relabel(workload: dict, process_name: str) -> dict:
    """A copy of `workload` labelled as process `process_name`'s, as a second workload for it."""
    workload = copy.deepcopy(workload)
    workload["metadata"].update(
        name=process_name, labels={**workload["metadata"]["labels"], "mandate.dev/process": process_name}
    )
    return workload


def migrate_gate(docs: list[dict]) -> dict:
    job = next(d for d in docs if d["kind"] == "Job" and d["metadata"]["name"] == "migrate")
    return copy.deepcopy(pod_spec(job)["initContainers"][0])


def manager_gate(docs: list[dict]) -> dict:
    return copy.deepcopy(pod_spec(next(w for w in workloads(docs) if is_manager(w)))["initContainers"][0])


SEEDED = [
    (one_workload_per_process, lambda d: [x for x in d if x["metadata"]["name"] != "scheduler"]),
    (colocated, lambda d: mutate(d, "executor", "Pod", lambda w: pod_spec(w).pop("affinity"))),
    (colocated, lambda d: mutate(d, "api", "Deployment", lambda w: pod_spec(w).pop("affinity"))),
    (colocated, lambda d: mutate(d, "model-gateway", "Deployment", lambda w: pod_spec(w).pop("affinity"))),
    (
        colocated,
        lambda d: mutate(
            d, "control-t1", "Job", lambda w: w["spec"]["template"]["metadata"]["labels"].pop("mandate.dev/colocate")
        ),
    ),
    (
        guaranteed,
        lambda d: mutate(
            d,
            "scheduler",
            "Deployment",
            lambda w: first_container(w)["resources"]["requests"].update(cpu="10m"),
        ),
    ),
    (default_deny, lambda d: [x for x in d if x["metadata"]["name"] != "default-deny"]),
    (
        default_deny,
        lambda d: mutate(d, "default-deny", "NetworkPolicy", lambda p: p["spec"].update(egress=[{}])),
    ),
    (
        egress_exact,
        lambda d: mutate(
            d,
            "egress-runtime",
            "NetworkPolicy",
            lambda p: p["spec"]["egress"].append({"to": [{"ipBlock": {"cidr": "0.0.0.0/0"}}]}),
        ),
    ),
    (
        egress_exact,
        lambda d: mutate(
            d,
            "egress-data",
            "NetworkPolicy",
            lambda p: p["spec"]["egress"].append(
                copy.deepcopy(policies(d)["egress-control"]["spec"]["egress"][1])
            ),
        ),
    ),
    (
        no_ingress,
        lambda d: mutate(d, "egress-executor", "NetworkPolicy", lambda p: p["spec"].update(ingress=[{}])),
    ),
    (
        hardened,
        lambda d: mutate(
            d,
            "control-t1",
            "Job",
            lambda w: first_container(w)["securityContext"].update(readOnlyRootFilesystem=False),
        ),
    ),
    (
        hardened,
        lambda d: mutate(d, "executor", "Pod", lambda w: pod_spec(w)["securityContext"].update(runAsUser=0)),
    ),
    (
        hardened,
        lambda d: mutate(
            d,
            "migrate",
            "Job",
            lambda w: pod_spec(w)["initContainers"][0]["securityContext"]["capabilities"].update(
                add=["NET_RAW"]
            ),
        ),
    ),
    (
        secrets_as_files,
        lambda d: mutate(
            d,
            "runtime",
            "Pod",
            lambda w: first_container(w)["env"].append(
                {"name": "PGPASSWORD", "valueFrom": {"secretKeyRef": {"name": "s", "key": "password"}}}
            ),
        ),
    ),
    (
        secrets_as_files,
        lambda d: mutate(
            d,
            "executor",
            "Pod",
            lambda w: first_container(w)["env"][0].update(value="postgres://u:pw@h/db"),
        ),
    ),
    (
        migration_first,
        lambda d: mutate(
            d,
            "migrate",
            "Job",
            lambda w: w["metadata"]["annotations"].update({"helm.sh/hook": "post-install"}),
        ),
    ),
    (
        migration_first,
        lambda d: mutate(d, "executor", "Pod", lambda w: first_container(w).update(args=["db", "migrate"])),
    ),
    (
        portable,
        lambda d: [*d, {"kind": "HelmChart", "apiVersion": "helm.cattle.io/v1", "metadata": {"name": "x"}}],
    ),
    (
        stub_sidecar,
        lambda d: mutate(
            render_with_stub(),
            "api",
            "Deployment",
            lambda w: next(c for c in pod_spec(w)["initContainers"] if c["name"] == "stub-issuer").update(
                args=["--bind", "0.0.0.0:8090"]
            ),
        ),
    ),
    (
        stub_sidecar,
        lambda d: mutate(
            render_with_stub(),
            "api",
            "Deployment",
            lambda w: pod_spec(w).update(
                initContainers=[c for c in pod_spec(w)["initContainers"] if c["name"] != "stub-issuer-ready"]
            ),
        ),
    ),
    (
        stub_sidecar,
        lambda d: mutate(
            render_with_stub(),
            "api",
            "Deployment",
            lambda w: next(c for c in pod_spec(w)["initContainers"] if c["name"] == "stub-issuer").pop(
                "restartPolicy"
            ),
        ),
    ),
    (
        api_contained,
        lambda d: mutate(
            d,
            "api",
            "Deployment",
            lambda w: first_container(w).update(
                args=["--bind", "0.0.0.0:8080", "--probe-bind", "127.0.0.1:8081"]
            ),
        ),
    ),
    (
        api_contained,
        lambda d: mutate(
            d, "api", "Deployment", lambda w: pod_spec(w)["volumes"][-1]["emptyDir"].pop("medium")
        ),
    ),
    (
        api_contained,
        lambda d: mutate(d, "api", "Deployment", lambda w: pod_spec(w)["initContainers"].pop()),
    ),
    (
        portable,
        lambda d: mutate(d, "executor", "Pod", lambda w: pod_spec(w).update(nodeSelector={"node": "pool-1"})),
    ),
    (
        one_workload_per_process,
        lambda d: [
            *d,
            relabel(next(x for x in d if x["metadata"]["name"] == "scheduler"), "runtime"),
        ],
    ),
    (
        gated,
        lambda d: mutate(
            d, MANAGER, "Deployment", lambda w: pod_spec(w)["initContainers"].__setitem__(0, migrate_gate(d))
        ),
    ),
    (
        gated,
        lambda d: mutate(
            d, "runtime", "Pod", lambda w: pod_spec(w)["initContainers"].__setitem__(0, manager_gate(d))
        ),
    ),
    (
        hardened,
        lambda d: mutate(
            d, MANAGER, "Deployment", lambda w: pod_spec(w).update(serviceAccountName="default")
        ),
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(d, MANAGER, "Role", lambda r: r["rules"][0]["resources"].append("secrets")),
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(d, MANAGER, "Role", lambda r: r["rules"][0]["verbs"].append("patch")),
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(
            d,
            MANAGER,
            "Role",
            lambda r: r["rules"].append({"apiGroups": [""], "resources": ["pods/exec"], "verbs": ["create"]}),
        ),
    ),
    (
        deploy_manager_contained,
        lambda d: [
            *d,
            {
                "kind": "ClusterRole",
                "apiVersion": "rbac.authorization.k8s.io/v1",
                "metadata": {"name": MANAGER},
                "rules": [POD_RULE],
            },
        ],
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(
            d,
            MANAGER,
            "RoleBinding",
            lambda b: b["subjects"].append(
                {"kind": "ServiceAccount", "name": "default", "namespace": "ws-ws1"}
            ),
        ),
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(
            d, "scheduler", "Deployment", lambda w: pod_spec(w).update(automountServiceAccountToken=True)
        ),
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(
            d,
            "egress-scheduler",
            "NetworkPolicy",
            lambda p: p["spec"]["egress"].append(
                {"to": [{"ipBlock": {"cidr": KUBE_API}}], "ports": [{"protocol": "TCP", "port": 6443}]}
            ),
        ),
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(
            d, MANAGER, "Deployment", lambda w: pod_spec(w)["initContainers"].__setitem__(0, migrate_gate(d))
        ),
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(
            d, "runtime", "Pod", lambda w: pod_spec(w).update(automountServiceAccountToken=True)
        ),
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(d, "executor", "Pod", lambda w: pod_spec(w).update(serviceAccountName=MANAGER)),
    ),
    (
        deploy_manager_contained,
        lambda d: mutate(
            d, MANAGER, "Deployment", lambda w: first_container(w)["volumeMounts"][-1].update(readOnly=False)
        ),
    ),
]


@pytest.mark.parametrize(
    ("check", "seed"), SEEDED, ids=[f"{c.__name__}-{i}" for i, (c, _) in enumerate(SEEDED)]
)
def test_each_seeded_bug_is_caught(check, seed):
    docs = render_all()
    assert check(docs) == []
    assert check(seed(docs)), f"{check.__name__} missed its seeded bug"


def policy_of(docs: list[dict], suffix: str = PODS_POLICY) -> str:
    return next(d["metadata"]["name"] for d in docs if d["kind"] == POLICY and d["metadata"]["name"].endswith(suffix))


def replace_variable(variable: str, how):
    def change(policy: dict) -> None:
        entry = next(v for v in policy["spec"]["variables"] if v["name"] == variable)
        entry["expression"] = json.dumps(how(json.loads(entry["expression"])))

    return change


def widen(variable: str, name: str, extra: str):
    """One process's list in a per-process map, widened by `extra`."""
    return replace_variable(variable, lambda lists: {**lists, name: [*lists.get(name, []), extra]})


def rewrite_rule(message: str, old: str, new: str):
    def change(policy: dict) -> None:
        rule = next(v for v in policy["spec"]["validations"] if v["message"] == message)
        rule["expression"] = rule["expression"].replace(old, new)

    return change


def loosen(message: str):
    """The rule of `message` made to admit everything, as `|| true` would."""
    return lambda policy: rewrite_rule(
        message, (rule := next(v for v in policy["spec"]["validations"] if v["message"] == message))["expression"],
        f"({rule['expression']}) || true",
    )(policy)


def drop_rule(message: str):
    def change(policy: dict) -> None:
        policy["spec"]["validations"] = [v for v in policy["spec"]["validations"] if v["message"] != message]

    return change


ADMISSION_SEEDED = [
    lambda d: [x for x in d if x["kind"] != POLICY],
    lambda d: [x for x in d if x["kind"] != POLICY_BINDING],
    lambda d: mutate(d, policy_of(d), POLICY, lambda p: p["spec"].update(failurePolicy="Ignore")),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        lambda p: p["spec"]["matchConditions"][0].update(
            expression="request.userInfo.username == 'system:serviceaccount:ws-ws1:default'"
        ),
    ),
    lambda d: mutate(d, policy_of(d), POLICY, lambda p: p["spec"].pop("matchConditions")),
    lambda d: mutate(d, policy_of(d), POLICY, widen("secrets", "runtime", "journal-role-mandate-control")),
    lambda d: mutate(d, policy_of(d), POLICY, widen("images", "runtime", "registry.example/shell:1")),
    lambda d: mutate(d, policy_of(d), POLICY, widen("claims", "runtime", "journal-1")),
    lambda d: mutate(d, policy_of(d), POLICY, widen("secrets", EXCHANGE, "journal-ca")),
    lambda d: mutate(d, policy_of(d), POLICY, widen("images", EXCHANGE, PLACEHOLDER.format("runtime"))),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        drop_rule("a pod the deployment manager makes runs exactly its own process's commands and arguments"),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        replace_variable(
            "images", lambda images: {n: sorted({i for v in images.values() for i in v}) for n in images}
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        replace_variable("projected", lambda projected: {n: projected[EXCHANGE] for n in projected}),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        widen(
            "projected", EXCHANGE, "mandate-vault|86400|token|292|namespace=metadata.namespace"
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        rewrite_rule(
            "a pod the deployment manager makes names no service account but the default, or a token exchange its own",
            "matches('^x([0-9a-f]{2}){1,64}$')",
            "matches('^x[0-9a-f]+$')",
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        rewrite_rule(
            "a pod the deployment manager makes runs exactly its own process's commands and arguments",
            "variables.commands[variables.process][c.name] ==",
            "variables.commands[variables.process][c.name] !=",
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        loosen("a pod the deployment manager makes reads no Secret through its environment or a pull secret"),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        loosen(
            "a pod the deployment manager makes mounts only memory, its own process's Secrets and projected token, or its claims read-only"
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        loosen(
            "a token exchange's pod names its own account in its environment once, beside its connection, which the exchange checks against each other"
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        loosen("a pod the deployment manager makes runs as no root user, adds no capability, and names no node or host alias"),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        loosen("a pod the deployment manager makes names no service account but the default, or a token exchange its own"),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        drop_rule("a pod the deployment manager makes carries exactly its process's egress labels"),
    ),
    lambda d: mutate(d, policy_of(d), POLICY_BINDING, lambda b: b["spec"].update(validationActions=["Warn"])),
    lambda d: mutate(
        d, policy_of(d), POLICY, drop_rule("a pod the deployment manager makes shares no host namespace")
    ),
]


@pytest.mark.parametrize(
    "seed", ADMISSION_SEEDED, ids=[f"admission-{i}" for i in range(len(ADMISSION_SEEDED))]
)
def test_each_admission_seeded_bug_is_caught(seed):
    docs = render_all()
    assert admission_holds(), "the unseeded chart already fails"
    assert next(admission_problems(seed(docs)), None), "deploy_manager_admission missed its seeded bug"


def test_roles_and_compose_seeded_bugs_are_caught():
    journal = render(*JOURNAL_ARGS, chart=JOURNAL_CHART)
    dropped = mutate(journal, "journal", "Cluster", lambda c: c["spec"]["managed"]["roles"].pop())
    assert roles_match(dropped)
    superuser = mutate(journal, "journal", "Cluster", lambda c: c["spec"].update(enableSuperuserAccess=True))
    assert roles_match(superuser)
    for change in (
        lambda c: c["services"]["control"].pop("read_only"),
        lambda c: c["services"]["migrate"].update(cap_add=["NET_ADMIN"]),
        lambda c: c["services"]["control"]["environment"].update(PGPASSWORD="x"),
        lambda c: c["services"]["journal-verify"]["networks"].append("egress"),
        lambda c: c["networks"].update(journal={}),
        lambda c: c["services"].pop("journal-verify"),
        lambda c: c["services"]["api"]["environment"].update(MANDATE_API_SECRET="x"),
        lambda c: c["services"].update({MANAGER: copy.deepcopy(c["services"]["api"])}),
    ):
        compose = copy.deepcopy(COMPOSE)
        change(compose)
        assert compose_matches(compose), change


def probe_binaries(docs: list[dict]) -> list[str]:
    """Every exec probe in the chart, the manager's pod templates included, names its binary at the
    image's root, where release images put it (`xtask/src/image.rs`, entrypoint `/<name>`) and the
    cell's images do too, so the same chart runs on both."""
    problems = []
    for w in workloads(docs):
        for c in containers(w):
            for kind in ("readinessProbe", "livenessProbe", "startupProbe"):
                command = c.get(kind, {}).get("exec", {}).get("command")
                if command and not re.fullmatch(r"/mandate-[a-z-]+", command[0]):
                    problems.append(f"{w['metadata']['name']}/{c['name']} {kind} runs {command[0]}")
            launch = c.get("command", [])
            if launch and "mandate-" in launch[0] and not re.fullmatch(r"/mandate-[a-z-]+", launch[0]):
                problems.append(f"{w['metadata']['name']}/{c['name']} runs {launch[0]}")
    return problems


def test_probes_name_the_binary_at_the_image_root():
    docs = render_all()
    assert probe_binaries(docs) == []
    assert any(
        c.get("readinessProbe", {}).get("exec") for w in workloads(docs) for c in containers(w)
    ), "no exec probe rendered, so nothing was checked"
    moved = mutate(
        docs,
        "executor",
        "Pod",
        lambda w: first_container(w)["readinessProbe"]["exec"]["command"].__setitem__(
            0, "/usr/local/bin/mandate-executord"
        ),
    )
    assert probe_binaries(moved)
    exchange = mutate(
        docs,
        EXCHANGE,
        "Pod",
        lambda w: first_container(w).update(command=["/usr/local/bin/mandate-token-exchange"]),
    )
    assert probe_binaries(exchange), "a launch outside the image root went unseen"


def test_the_journal_reaches_the_kubernetes_api_by_ipv4_hosts_only():
    """Whatever names the Kubernetes API's addresses, values or the cluster lookup, the policy takes
    only single IPv4 hosts: an IPv6 address read as `/32` would cover a vast range. The schema
    refuses them in values; the template refuses them from any source, shown with the schema
    skipped, since a rendering without a cluster cannot fake the lookup."""
    k3d = ("-f", str(CELL / "values" / "k3d-journal-db.yaml"))
    assert helm(JOURNAL_CHART, *JOURNAL_ARGS).returncode == 0
    for cidrs in (["fd00::1/32"], ["10.43.0.1/32", "fd00::1/32"], ["10.0.0.0/8"], ["0.0.0.0/0"], ["10.43.0.1"]):
        api = json.dumps({"cidrs": cidrs, "ports": [443]})
        assert helm(JOURNAL_CHART, *k3d, "--set-json", f"kubeApi={api}").returncode != 0, cidrs
        skipped = helm(JOURNAL_CHART, *k3d, "--skip-schema-validation", "--set-json", f"kubeApi={api}")
        assert skipped.returncode != 0 and "Kubernetes API" in skipped.stderr, cidrs
    unknown = helm(JOURNAL_CHART, *k3d)
    assert unknown.returncode != 0 and "Kubernetes API" in unknown.stderr
