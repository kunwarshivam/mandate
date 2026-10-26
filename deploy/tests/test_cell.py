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
    "executor": {
        ("postgres", "TCP", 5432),
        ("vault", "TCP", 8200),
        ("broker", "TCP", 443),
        ("market-data", "TCP", 443),
    },
    "token-exchange": {("vault", "TCP", 8200), ("token-host", "TCP", 443)},
    "scheduler": {("postgres", "TCP", 5432), ("time", "UDP", 123)},
    "dispatcher": {("postgres", "TCP", 5432)},
    "model-gateway": {
        ("postgres", "TCP", 5432),
        ("vault", "TCP", 8200),
        ("model-gateway", "TCP", 8080),
        ("model-providers", "TCP", 443),
    },
    "exporter": {("postgres", "TCP", 5432), ("object-storage", "TCP", 443), ("timestamping", "TCP", 443)},
    "data": {("market-data", "TCP", 443)},
    "deploy-manager": {("postgres", "TCP", 5432), ("kube-api", "TCP", 6443)},
    "role-provisioner": {("postgres", "TCP", 5432), ("vault", "TCP", 8200)},
    "connect-finish": {("postgres", "TCP", 5432), ("vault", "TCP", 8200)},
}
# The API server's endpoint the tests give the deployment manager (DEC-1172), as `cargo xtask cell
# up` gives it the cell's own; the manager is the one process that may reach it.
KUBE_API = "192.0.2.200/32"
MANAGER = "deploy-manager"
TEMPLATES = "deploy-manager-templates"
HOOK = "pre-install,pre-upgrade"
# The model gateway's workspace and deployment, which its image requires (E21-62); no budget, so a
# rendering names none unless a test sets one (DEC-2326 item 2).
GATEWAY = {"workspace": "01KDEM0000000000000000000W", "deployment": "cell-test"}
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


# The one recipient the dispatcher's email channel mails, in DEC-710 item 4's form (DEC-2516).
RECIPIENT = "demo_owner"


def every_image() -> list[str]:
    sets = [
        f"images.{image}={PLACEHOLDER.format(image)}"
        for image in dict.fromkeys(p["image"] for p in PROCESSES)
        if image != "postgres"
    ]
    sets += [
        f"data.instruments[{i}].{key}={value}"
        for i, instrument in enumerate(INSTRUMENTS)
        for key, value in instrument.items()
    ]
    sets.append(f"dispatcher.recipient={RECIPIENT}")
    sets += [f"modelGateway.{key}={value}" for key, value in GATEWAY.items()]
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
    renders, `control` unless it names another (DEC-2035). The connect finish renders only with
    `api.connectFinish`, which `connect_finish` checks (DEC-2348 item 3)."""
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
        if p["name"] == FINISH and not found:
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
# Variables that name an address and hold no credential, each with the one value it may take: the
# token exchange's token URL, pointed at a dev or ci cell's fixture pod (DEC-1176 item 5).
ADDRESS_NAMES = {"MANDATE_TOKEN_URL": "http://oauth-fixture:8091/alpaca/oauth/token"}


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
                address = ADDRESS_NAMES.get(e["name"]) == e.get("value")
                if SECRET_KEY.search(e["name"]) and e["name"] not in FILE_PATH_NAMES and not address:
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
            .replace("{hosted_workspace}", HOSTED)
            .replace("{data_instruments}", INSTRUMENTS_ARG)
            .replace("{agent}", "demo-agent")
            .replace("{connection}", "demo-paper")
            .replace("{owner}", "demo-owner")
            for x in p["args"]
        ]
        assert c["readinessProbe"]["exec"]["command"] == [x.replace("{probe}", "readyz") for x in p["probe"]]
        assert c["livenessProbe"]["exec"]["command"] == [x.replace("{probe}", "livez") for x in p["probe"]]


HOSTED = "01KDEM0000000000000000000W"
SPY_ASSET = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415"
QQQ_ASSET = "eb82ffb0-efd6-4f4e-9307-8fead28e04f6"
# The cell's two recorded instruments: the demo agent's SPY and the second agent's QQQ, which the
# trade loop runs on (`demo/instrument.json`, `demo/web-instrument.json`).
INSTRUMENTS = [
    {"symbol": "SPY", "assetId": SPY_ASSET, "anchor": "2026-10-23=150"},
    {"symbol": "QQQ", "assetId": QQQ_ASSET, "anchor": "2026-10-23=140"},
]
INSTRUMENTS_ARG = ",".join(f"{i['symbol']}:{i['assetId']}:{i['anchor']}" for i in INSTRUMENTS)
SCHEDULER = ("--set", f"images.scheduler={PLACEHOLDER.format('scheduler')}")


def test_the_scheduler_writes_the_clock_stream_the_manager_s_processes_read():
    """One scheduler per workspace writes `clock:{workspace_id}` (journal spec §2, E21-29), and the
    runtime and executor the manager starts read the clock stream of the manager's own `--workspace`
    (`MANDATE_WORKSPACE`), so the scheduler is named the same workspace: the hosted one,
    `api.workspace`, never the namespace's short name. It asks no time server (`egress.time` is
    empty in the cell), so every measurement is journaled out of tolerance (AGENTS.md rule 3)."""
    docs = render_all(*SCHEDULER)
    by_name = {w["metadata"]["name"]: w for w in workloads(docs)}
    scheduler = by_name["scheduler"]
    assert scheduler["kind"] == "Deployment" and scheduler["spec"]["replicas"] == 1
    c = first_container(scheduler)
    assert c["image"] == PLACEHOLDER.format("scheduler")
    assert c["args"] == ["run", "--workspace", HOSTED, "--probe", "/tmp/process.sock"]
    manager_args = first_container(by_name[MANAGER])["args"]
    assert manager_args[manager_args.index("--workspace") + 1] == HOSTED
    dsn = next(e["value"] for e in c["env"] if e["name"] == "MANDATE_JOURNAL_DSN")
    assert dsn.startswith("postgres://mandate_scheduler@")
    for k, asked in (("readinessProbe", "readyz"), ("livenessProbe", "livez")):
        assert c[k]["exec"]["command"] == ["/mandate-scheduler", "probe", asked, "--probe", "/tmp/process.sock"]
    volumes = {v["name"] for v in pod_spec(scheduler).get("volumes", [])}
    assert not [v for v in volumes if "vault" in v or v == "artifacts"], volumes
    for check in (hardened, gated, default_deny, secrets_as_files, migration_first):
        assert check(docs) == [], check.__name__
    assert egress_exact(docs) == []


def test_the_scheduler_never_renders_for_no_hosted_workspace():
    """Naming the scheduler's image without the hosted workspace fails the render, rather than
    ticking a clock stream no process reads (AGENTS.md rule 3)."""
    done = helm(CHART, *SCHEDULER, "--set", "api.workspace=")
    assert done.returncode != 0
    assert "api.workspace" in done.stderr and "scheduler" in done.stderr


def dispatcher_config(docs: list[dict]) -> dict:
    """The dispatcher's configuration file, as the ConfigMap it mounts holds it."""
    config = next(d for d in docs if d["kind"] == "ConfigMap" and d["metadata"]["name"] == "dispatcher-config")
    return json.loads(config["data"]["config.json"])


def test_the_dispatcher_follows_the_hosted_workspace_and_mails_only_its_one_recipient():
    """One dispatcher per workspace writes `ntf:{workspace_id}` (DEC-2500, DEC-2516): the hosted
    workspace, whose control stream names the streams it follows, as its own journal role, with
    Postgres its one destination. Its configuration file is mounted read-only and holds no secret
    and no address: one email recipient, web push off (DEC-2504), paper, and the build its image is
    pinned by. The expected file is written out here, not read from the chart."""
    docs = render_all()
    by_name = {w["metadata"]["name"]: w for w in workloads(docs)}
    dispatcher = by_name["dispatcher"]
    assert dispatcher["kind"] == "Deployment" and dispatcher["spec"]["replicas"] == 1
    c = first_container(dispatcher)
    assert c["image"] == PLACEHOLDER.format("dispatcher")
    assert c["args"] == ["run", "--config", "/etc/mandate/dispatcher/config.json", "--probe", "/tmp/process.sock"]
    dsn = next(e["value"] for e in c["env"] if e["name"] == "MANDATE_JOURNAL_DSN")
    assert dsn.startswith("postgres://mandate_dispatcher@")
    for k, asked in (("readinessProbe", "readyz"), ("livenessProbe", "livez")):
        assert c[k]["exec"]["command"] == ["/mandate-dispatcher", "probe", asked, "--probe", "/tmp/process.sock"]
    mount = next(m for m in c["volumeMounts"] if m["name"] == "dispatcher-config")
    assert mount == {"name": "dispatcher-config", "mountPath": "/etc/mandate/dispatcher", "readOnly": True}
    volumes = {v["name"]: v for v in pod_spec(dispatcher).get("volumes", [])}
    assert volumes["dispatcher-config"]["configMap"]["name"] == "dispatcher-config"
    assert not [v for v in volumes if "vault" in v or v == "artifacts"], volumes
    assert dispatcher_config(docs) == {
        "workspace": HOSTED,
        "subjects": [],
        "audience": [{"recipient": RECIPIENT, "channel": "email"}],
        "app_origin": "https://localhost:4317",
        "environment": "paper",
        "build": "sha256:" + "0" * 64,
        "web_push": False,
        "push_allowlist": [],
        "vapid_subject": None,
    }
    for check in (hardened, gated, default_deny, secrets_as_files, migration_first):
        assert check(docs) == [], check.__name__
    assert egress_exact(docs) == []


def test_a_local_dispatcher_takes_its_binary_digest_as_its_build():
    """A local build has no digest in its name, so `cargo xtask cell up` sets the binary's own as
    `dispatcher.build`; the file carries that, and a local image without it fails the render."""
    local = ("--set", "images.dispatcher=mandate.local/dispatcher:0123456789ab")
    build = "sha256:" + "a" * 64
    docs = render_all(*local, "--set", f"dispatcher.build={build}")
    assert dispatcher_config(docs)["build"] == build
    done = helm(CHART, "-f", str(CELL / "values" / "k3d.yaml"), *every_image(), *local)
    assert done.returncode != 0 and "dispatcher.build" in done.stderr


def test_the_dispatcher_never_renders_for_what_it_cannot_honour():
    """No hosted workspace, no recipient, a recipient not in DEC-710 item 4's form, no app origin,
    or a backtest release fails the render rather than starting a dispatcher that sends nothing or
    to no one (AGENTS.md rule 3)."""
    base = ("-f", str(CELL / "values" / "k3d.yaml"), *every_image())
    for why, extra in {
        "no hosted workspace": ("--set", "api.workspace="),
        "no recipient": ("--set", "dispatcher.recipient="),
        "a recipient not in its form": ("--set", "dispatcher.recipient=Demo-Owner"),
        "no app origin": ("--set", "api.appOrigin="),
        "backtest": ("--set", "environment=backtest"),
    }.items():
        assert helm(CHART, *base, *extra).returncode != 0, why


DATA = ("--set", f"images.data={PLACEHOLDER.format('data')}")


def test_the_data_process_stores_the_recorded_series_where_the_runtime_reads_it():
    """The data process writes the deployed instrument's daily bars from the cell's recorded series
    into the artifact store, which it alone besides control and the API mounts writable and the
    runtime mounts read-only; it holds no journal role and no vault token, and reaches no host in
    the cell (`egress.market-data` is empty)."""
    docs = render_all(*DATA)
    by_name = {w["metadata"]["name"]: w for w in workloads(docs)}
    data = by_name["data"]
    c = first_container(data)
    assert c["image"] == PLACEHOLDER.format("data")
    assert c["args"] == [
        "run",
        "--store",
        "/var/lib/mandate/artifacts/datasets",
        "--artifacts",
        "/var/lib/mandate/artifacts",
        "--instruments",
        INSTRUMENTS_ARG,
        "--probe",
        "/tmp/process.sock",
    ]
    assert not [e for e in c["env"] if e["name"] in ("MANDATE_JOURNAL_DSN", "PGPASSFILE")]
    mounts = {m["mountPath"]: m.get("readOnly", False) for m in c["volumeMounts"]}
    assert mounts.get("/var/lib/mandate/artifacts") is False, mounts
    volumes = {v["name"] for v in pod_spec(data).get("volumes", [])}
    assert not [v for v in volumes if "vault" in v or "journal" in v or v.startswith("pg")], volumes
    for k, asked in (("readinessProbe", "readyz"), ("livenessProbe", "livez")):
        assert c[k]["exec"]["command"] == ["/mandate-datad", "probe", asked, "--probe", "/tmp/process.sock"]
    for check in (hardened, gated, default_deny, colocated, egress_exact):
        assert check(docs) == [], check.__name__


def test_the_data_process_never_renders_without_its_series_or_outside_the_cell(tmp_path: Path):
    """Naming the data image without the series fails the render, as does an instrument missing a
    field or a symbol listed twice, and only the local cell's values name one: the series is a
    recording, never market data."""
    k3d = ("-f", str(CELL / "values" / "k3d.yaml"), *DATA)
    none = tmp_path / "none.yaml"
    none.write_text("data:\n  instruments: []\n")
    done = helm(CHART, *k3d, "-f", str(none))
    assert done.returncode != 0 and "data.instruments, the instruments" in done.stderr, done.stderr
    for unset, said in (
        ("data.instruments[0].symbol=", "/data/instruments/0/symbol"),
        ("data.instruments[1].assetId=", "/data/instruments/1/assetId"),
        ("data.instruments[1].anchor=", "/data/instruments/1/anchor"),
        ("data.instruments[0].anchor=150", "/data/instruments/0/anchor"),
        ("data.instruments[1].symbol=SPY", "data.instruments lists SPY twice"),
    ):
        done = helm(CHART, *k3d, "--set", unset)
        assert done.returncode != 0 and said in done.stderr, f"{unset}: {done.stderr}"
    for values in ("k3s.yaml", "managed.yaml"):
        done = helm(CHART, "-f", str(CELL / "values" / values), *DATA)
        assert done.returncode != 0 and "data.instruments" in done.stderr, f"{values}: only the local cell names a series"


GATEWAY_BUDGETS = ("MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD", "MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD")


def gateway_env(docs: list[dict]) -> dict[str, str]:
    gateway = next(w for w in workloads(docs) if w["metadata"]["name"] == "model-gateway")
    return {e["name"]: e.get("value", "") for e in first_container(gateway).get("env", [])}


def test_the_model_gateway_serves_the_recording_and_names_no_budget_of_its_own():
    """E21-62, DEC-2326 items 2 and 3: the model gateway runs `--recorded` over the bundle its image
    carries and never a live start; its workspace and deployment are required; its budgets render
    only where values name them, which only the local cell's do, and the chart refuses them where
    images are not the cell's own."""
    docs = render_all()
    gateway = next(w for w in workloads(docs) if w["metadata"]["name"] == "model-gateway")
    args = first_container(gateway)["args"]
    assert args[args.index("--recorded") + 1] == f"{RESEARCH}/research.json"
    assert not any(a.startswith("--registry") for a in args), args
    env = gateway_env(docs)
    assert env["MANDATE_GATEWAY_WORKSPACE"] == GATEWAY["workspace"]
    assert env["MANDATE_GATEWAY_DEPLOYMENT"] == GATEWAY["deployment"]
    assert (env[GATEWAY_BUDGETS[0]], env[GATEWAY_BUDGETS[1]]) == ("50", "20"), "the recorded bundle's caps"
    mount = next(m for m in first_container(gateway)["volumeMounts"] if m["name"] == "artifacts")
    assert mount["readOnly"] is True
    unnamed = render_all("--set", "modelGateway.workspaceMonthlyUsd=", "--set", "modelGateway.deploymentMonthlyUsd=")
    assert not set(GATEWAY_BUDGETS) & set(gateway_env(unnamed)), "no budget without the values naming one"
    for unset in ("modelGateway.workspace=", "modelGateway.deployment="):
        done = helm(CHART, "-f", str(CELL / "values" / "k3d.yaml"), *every_image(), "--set", unset)
        assert done.returncode != 0 and "is required with images.model-gateway" in done.stderr, unset
    hosted = helm(CHART, "-f", str(CELL / "values" / "k3d.yaml"), *every_image(), "--set", "localImages=false")
    assert hosted.returncode != 0 and "named only by the local cell's values" in hosted.stderr, hosted.stderr


def test_only_the_local_cell_names_the_model_gateway_s_budgets():
    """DEC-2326 item 2, DEC-2396: no values file but the local cell's names a model budget, and the
    local cell's gateway serves the workspace its API hosts."""
    for values in sorted((CELL / "values").glob("*.yaml")) + sorted(
        (CELL.parent / "gitops").rglob("*.yaml")
    ):
        if values.name == "k3d.yaml":
            continue
        text = values.read_text()
        assert "MonthlyUsd" not in text, f"{values} names a model budget"
    defaults = (CHART / "values.yaml").read_text()
    for budget in ("workspaceMonthlyUsd", "deploymentMonthlyUsd"):
        assert f'  {budget}: ""\n' in defaults, f"the chart defaults {budget}"
    k3d = (CELL / "values" / "k3d.yaml").read_text()
    workspaces = {
        block: re.search(rf"^{block}:\n(?:  .*\n)*?  workspace: (\S+)$", k3d, re.MULTILINE).group(1)
        for block in ("api", "modelGateway")
    }
    assert workspaces["api"] == workspaces["modelGateway"], workspaces


def test_the_local_cell_records_both_demo_agents_instruments():
    """The local cell's values list SPY and QQQ, each with the asset id its demo agent's envelope
    pins, so the second agent's trade loop has bars, closes and marks as the first's has."""
    docs = render("-f", str(CELL / "values" / "k3d.yaml"), *DATA)
    data = next(w for w in workloads(docs) if w["metadata"]["name"] == "data")
    args = first_container(data)["args"]
    assert args[args.index("--instruments") + 1] == INSTRUMENTS_ARG
    for name, instrument in (("instrument.json", INSTRUMENTS[0]), ("web-instrument.json", INSTRUMENTS[1])):
        pinned = json.loads((CELL / "demo" / name).read_text())
        assert (pinned["symbol"], pinned["instrument_id"]) == (instrument["symbol"], instrument["assetId"]), name


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


API_VAULT_DIR = "/run/mandate/vault"
API_VAULT_SOURCES = {
    "defaultMode": 288,
    "sources": [
        {
            "secret": {
                "name": "vault-approle-api",
                "items": [
                    {"key": "role-id", "path": "api.role-id"},
                    {"key": "secret-id", "path": "api.secret-id"},
                ],
            }
        },
        {"configMap": {"name": "vault-ca", "items": [{"key": "ca.crt", "path": "ca.crt"}]}},
    ],
}
API_VAULT_COPY = [
    "install",
    "-m",
    "0600",
    "/run/secrets/vault-api/api.role-id",
    "/run/secrets/vault-api/api.secret-id",
    "/run/secrets/vault-api/ca.crt",
    f"{API_VAULT_DIR}/",
]

APPROLE_NAME = re.compile(r'"(vault-approle-[a-z0-9-]+)"')


def api_vault(docs: list[dict], on: bool) -> list[str]:
    """The API's vault binding (DEC-2338), exactly, when `on`, and none of it otherwise: its
    AppRole's two keys and the vault's CA projected read-only into one source directory, copied by
    one init container into a memory-backed directory at 0600 with nothing else in it, which the API
    mounts read-only and names in `MANDATE_API_VAULT_DIR`, never a credential in its environment;
    and its egress to the vault on 8200 alone. No other pod names its AppRole, and the API pod names
    no other: the connection manager's is the connect finish's alone (DEC-2348 item 3)."""
    problems = []
    for w in [w for w in workloads(docs) if w["metadata"]["name"] == "api"]:
        spec = pod_spec(w)
        api = next(c for c in spec["containers"] if c["name"] == "api")
        env = {e["name"]: e.get("value") for e in api.get("env", [])}
        volumes = {v["name"]: v for v in spec.get("volumes", [])}
        vaulted = {n for n, v in volumes.items() if n in ("vault", "vault-sources") or "projected" in v}
        copies = [c for c in spec.get("initContainers", []) if c["name"] == "vault-credentials"]
        mounts = {m["mountPath"]: m for m in api.get("volumeMounts", [])}
        labels = w["spec"]["template"]["metadata"]["labels"]
        holders = sorted(
            (m["name"], c["name"], m.get("readOnly", False))
            for c in spec.get("containers", []) + spec.get("initContainers", [])
            for m in c.get("volumeMounts", [])
            if m["name"] in ("vault", "vault-sources")
        )
        wanted = [("vault", "api", True), ("vault", "vault-credentials", False), ("vault-sources", "vault-credentials", True)]
        if holders != (wanted if on else []):
            problems.append(f"the api pod's vault volumes are mounted by {holders}")
        rules = policies(docs)["egress-api"]["spec"]["egress"]
        if any("to" not in r or "ports" not in r for r in rules):
            problems.append(f"an api egress rule names no peer or no port, so it reaches the vault: {rules}")
        reaching = [r for r in rules if '"vault"' in json.dumps(r) or any(p.get("port") == 8200 for p in r.get("ports", []))]
        labelled = labels.get("mandate.dev/egress-vault")
        if not on:
            if vaulted or copies or API_VAULT_DIR in mounts or "MANDATE_API_VAULT_DIR" in env:
                problems.append(f"the api is bound to the vault while off: {sorted(vaulted)}")
            if reaching or labelled:
                problems.append("the api reaches the vault while off")
            continue
        if vaulted != {"vault", "vault-sources"}:
            problems.append(f"the api's vault volumes are {sorted(vaulted)}")
        if volumes.get("vault-sources", {}).get("projected") != API_VAULT_SOURCES:
            problems.append(f"the api's vault sources are {volumes.get('vault-sources')}")
        if volumes.get("vault", {}).get("emptyDir", {}).get("medium") != "Memory":
            problems.append(f"the api's vault directory is {volumes.get('vault')}")
        if [c.get("command") for c in copies] != [API_VAULT_COPY]:
            problems.append(f"the api's credentials are copied by {[c.get('command') for c in copies]}")
        copy_mounts = sorted((m["name"], m["mountPath"], m.get("readOnly", False)) for c in copies for m in c["volumeMounts"])
        if copy_mounts != [("vault", API_VAULT_DIR, False), ("vault-sources", "/run/secrets/vault-api", True)]:
            problems.append(f"the copy mounts {copy_mounts}")
        mounted = mounts.get(API_VAULT_DIR, {})
        if mounted.get("name") != "vault" or mounted.get("readOnly") is not True:
            problems.append(f"the api mounts its vault directory as {mounted}")
        if any(m["name"] == "vault-sources" for m in api.get("volumeMounts", [])):
            problems.append("the api mounts the AppRole's Secret itself")
        if env.get("MANDATE_API_VAULT_DIR") != API_VAULT_DIR or env.get("MANDATE_API_VAULT_ADDRESS") != api_vault_address(labels):
            problems.append(f"the api's vault settings are {env.get('MANDATE_API_VAULT_DIR')}, {env.get('MANDATE_API_VAULT_ADDRESS')}")
        if any(n.startswith("MANDATE_API_VAULT") and n not in ("MANDATE_API_VAULT_DIR", "MANDATE_API_VAULT_ADDRESS") for n in env):
            problems.append(f"the api's environment names another vault setting: {sorted(env)}")
        if reaching != [api_vault_rule(labels)]:
            problems.append(f"the api's egress to the vault is {reaching}")
        if labelled != "true":
            problems.append("the api's pod carries no egress-vault label, so the vault refuses it")
    holders = sorted(
        (w["kind"], w["metadata"]["name"], name)
        for w in workloads(docs)
        for name in APPROLE_NAME.findall(json.dumps(w))
        if (w["metadata"]["name"], name)
        not in ((PROVISIONER, "vault-approle-provisioner"), (FINISH, "vault-approle-connection-manager"))
    )
    if holders != ([("Deployment", "api", "vault-approle-api")] if on else []):
        problems.append(f"the AppRoles are named by {holders}")
    return problems


FINISH = "connect-finish"
FINISH_VAULT_SOURCES = {
    "defaultMode": 288,
    "sources": [
        {
            "secret": {
                "name": "vault-approle-connection-manager",
                "items": [
                    {"key": "role-id", "path": "connection-manager.role-id"},
                    {"key": "secret-id", "path": "connection-manager.secret-id"},
                ],
            }
        },
        {"configMap": {"name": "vault-ca", "items": [{"key": "ca.crt", "path": "ca.crt"}]}},
    ],
}
FINISH_VAULT_COPY = [
    "install",
    "-m",
    "0600",
    "/run/secrets/vault-connection-manager/connection-manager.role-id",
    "/run/secrets/vault-connection-manager/connection-manager.secret-id",
    "/run/secrets/vault-connection-manager/ca.crt",
    f"{API_VAULT_DIR}/",
]
FINISH_ENV = {
    "MANDATE_JOURNAL_DSN",
    "PGPASSFILE",
    "HOME",
    "MANDATE_DEPLOYMENT_TIER",
    "MANDATE_API_DATABASE_URL",
    "MANDATE_API_WORKSPACE",
    "MANDATE_FINISH_VAULT_ADDRESS",
    "MANDATE_FINISH_VAULT_DIR",
}


def connect_finish(docs: list[dict], on: bool, workspace: str) -> list[str]:
    """DEC-2348 item 3: with `api.connectFinish` the connect finish is one Deployment, the API's
    image as `finish`, holding the connection manager's AppRole and the vault's CA alone, copied
    owner-only, and no Kubernetes credential; it reads the hosted workspace `workspace`'s control
    stream as its own journal role; its egress is the journal and its vault on 8200, nothing else.
    Nothing but it names that AppRole, the API pod least of all, which holds no
    `connection-manager.*` file in any volume, copy or mount. Off, none of it renders."""
    problems = []
    found = [w for w in workloads(docs) if w["metadata"]["name"] == FINISH]
    named = sorted(
        (d["kind"], d["metadata"]["name"])
        for d in workloads(docs)
        for _ in range(json.dumps(d).count('"vault-approle-connection-manager"'))
    )
    api = [w for w in workloads(docs) if w["metadata"]["name"] == "api"]
    if any("connection-manager" in json.dumps(w) for w in api):
        problems.append("the api pod names the connection manager's AppRole or its files")
    if not on:
        if found or named:
            problems.append(f"the connect finish renders while off: {[w['kind'] for w in found]}, {named}")
        return problems
    if named != [("Deployment", FINISH)]:
        problems.append(f"the connection manager's AppRole is named by {named}")
    if [w["kind"] for w in found] != ["Deployment"]:
        return problems + [f"the connect finish renders as {[w['kind'] for w in found]}"]
    w = found[0]
    spec = pod_spec(w)
    labels = w["spec"]["template"]["metadata"]["labels"]
    if w["spec"].get("replicas") != 1:
        problems.append(f"the connect finish runs {w['spec'].get('replicas')} replicas")
    if spec.get("terminationGracePeriodSeconds") != 30:
        problems.append(f"the connect finish's grace is {spec.get('terminationGracePeriodSeconds')}, not the 30 seconds its drain fits")
    if spec.get("automountServiceAccountToken") is not False or spec.get("serviceAccountName", "default") != "default":
        problems.append("the connect finish's pod holds a service-account token or an account of its own")
    main = next((c for c in spec["containers"] if c["name"] == FINISH), None)
    if main is None or len(spec["containers"]) != 1:
        return problems + [f"the connect finish's containers are {[c['name'] for c in spec['containers']]}"]
    if main.get("args") != ["finish"] or main.get("command"):
        problems.append(f"the connect finish starts as {main.get('command')} {main.get('args')}")
    if api and main.get("image") != next(c for c in pod_spec(api[0])["containers"] if c["name"] == "api").get("image"):
        problems.append(f"the connect finish runs {main.get('image')}, not the API's image")
    env = {e["name"]: e for e in main.get("env", [])}
    if set(env) != FINISH_ENV or any("valueFrom" in e for e in env.values()) or main.get("envFrom"):
        problems.append(f"the connect finish's environment is {sorted(env)}")
    value = lambda name: env.get(name, {}).get("value", "")
    if not value("MANDATE_API_DATABASE_URL").startswith("postgres://mandate_connect_finish@"):
        problems.append("the connect finish reaches the journal as another role")
    if value("MANDATE_API_WORKSPACE") != workspace:
        problems.append(f"the connect finish finishes the workspace {value('MANDATE_API_WORKSPACE')}")
    if value("MANDATE_FINISH_VAULT_DIR") != API_VAULT_DIR or value("MANDATE_FINISH_VAULT_ADDRESS") != api_vault_address(labels):
        problems.append(f"the connect finish's vault is {value('MANDATE_FINISH_VAULT_ADDRESS')} {value('MANDATE_FINISH_VAULT_DIR')}")
    volumes = {v["name"]: v for v in spec.get("volumes", [])}
    secrets = sorted(
        [v["secret"]["secretName"] for v in volumes.values() if "secret" in v]
        + [x["secret"]["name"] for v in volumes.values() if "projected" in v for x in v["projected"]["sources"] if "secret" in x]
    )
    if secrets != ["journal-ca", "journal-role-mandate-connect-finish", "vault-approle-connection-manager"]:
        problems.append(f"the connect finish's pod mounts the Secrets {secrets}")
    projected = {n: sorted(k for x in v["projected"]["sources"] for k in x) for n, v in volumes.items() if "projected" in v}
    if any(k not in ("name", "emptyDir", "secret", "projected") for v in volumes.values() for k in v) or projected != {"vault-sources": ["configMap", "secret"]}:
        problems.append(f"the connect finish's pod has the volumes {sorted(volumes)}, projecting {projected}")
    if volumes.get("vault-sources", {}).get("projected") != FINISH_VAULT_SOURCES:
        problems.append(f"the connect finish's vault sources are {volumes.get('vault-sources')}")
    if volumes.get("vault", {}).get("emptyDir", {}).get("medium") != "Memory":
        problems.append(f"the connect finish's vault directory is {volumes.get('vault')}")
    copies = [c for c in spec.get("initContainers", []) if c["name"] == "vault-credentials"]
    if [c.get("command") for c in copies] != [FINISH_VAULT_COPY]:
        problems.append(f"the connect finish's credentials are copied by {[c.get('command') for c in copies]}")
    holders = sorted(
        (m["name"], c["name"], m.get("mountPath"), m.get("readOnly", False))
        for c in spec.get("containers", []) + spec.get("initContainers", [])
        for m in c.get("volumeMounts", [])
        if m["name"] in ("vault", "vault-sources")
    )
    wanted = [
        ("vault", FINISH, API_VAULT_DIR, True),
        ("vault", "vault-credentials", API_VAULT_DIR, False),
        ("vault-sources", "vault-credentials", "/run/secrets/vault-connection-manager", True),
    ]
    if holders != wanted:
        problems.append(f"the connect finish's vault volumes are mounted by {holders}")
    egress = sorted(k for k, v in labels.items() if k.startswith("mandate.dev/egress-") and v == "true")
    if egress != ["mandate.dev/egress-postgres", "mandate.dev/egress-vault"]:
        problems.append(f"the connect finish's pod carries the egress labels {egress}")
    rules = policies(docs)[f"egress-{FINISH}"]["spec"]["egress"]
    reaching = [r for r in rules if '"vault"' in json.dumps(r) or any(p.get("port") == 8200 for p in r.get("ports", []))]
    if reaching != [api_vault_rule(labels)] or len(rules) != 3 or any("to" not in r or "ports" not in r for r in rules):
        problems.append(f"the connect finish's egress is {rules}")
    return problems


def test_the_connect_finish_alone_holds_the_connection_managers_approle():
    """DEC-2348 item 3: the connect finish runs in its own pod with the connection manager's
    AppRole alone, the API pod holds no `connection-manager.*` file, and off, nothing renders."""
    off = render_all("--set", "api.vaultBinding=true")
    on = render_all("--set", "api.vaultBinding=true", "--set", "api.connectFinish=true")
    workspace = api_env(on)["MANDATE_API_WORKSPACE"]
    assert connect_finish(off, on=False, workspace=workspace) == []
    assert connect_finish(on, on=True, workspace=workspace) == []
    assert api_vault(on, on=True) == []
    for check in (hardened, gated, secrets_as_files, default_deny, approle_copied):
        assert check(on) == [], check.__name__
    assert connect_finish(on, on=False, workspace=workspace), "the finish while off is caught"
    assert connect_finish(off, on=True, workspace=workspace), "no finish while on is caught"
    main = lambda w: next(c for c in pod_spec(w)["containers"] if c["name"] in (FINISH, PROVISIONER))
    volume = lambda w, n: next(v for v in pod_spec(w)["volumes"] if v["name"] == n)
    copier = lambda w: next(c for c in pod_spec(w)["initContainers"] if c["name"] == "vault-credentials")
    manager = copy.deepcopy(FINISH_VAULT_SOURCES["sources"][0])
    seeds = [
        ("Deployment", FINISH, lambda w: w["spec"].update(replicas=2)),
        ("Deployment", FINISH, lambda w: pod_spec(w).update(terminationGracePeriodSeconds=60)),
        ("Deployment", FINISH, lambda w: pod_spec(w).pop("terminationGracePeriodSeconds")),
        ("Deployment", FINISH, lambda w: pod_spec(w).update(automountServiceAccountToken=True)),
        ("Deployment", FINISH, lambda w: main(w).update(args=["--bind", "127.0.0.1:8080"])),
        ("Deployment", FINISH, lambda w: main(w).update(image="other@sha256:" + "0" * 64)),
        ("Deployment", FINISH, lambda w: main(w)["env"].append({"name": "MANDATE_API_VAULT_DIR", "value": API_VAULT_DIR})),
        ("Deployment", FINISH, lambda w: next(e for e in main(w)["env"] if e["name"] == "MANDATE_API_DATABASE_URL").update(value="postgres://mandate_api@journal-rw:5432/mandate")),
        ("Deployment", FINISH, lambda w: next(e for e in main(w)["env"] if e["name"] == "MANDATE_API_WORKSPACE").update(value="01JB3K6F0C9R2V7N4M8Q1T5W3Z")),
        ("Deployment", FINISH, lambda w: next(e for e in main(w)["env"] if e["name"] == "MANDATE_FINISH_VAULT_ADDRESS").update(value="https://vault.example.com:8200")),
        ("Deployment", FINISH, lambda w: volume(w, "vault-sources")["projected"]["sources"][0]["secret"].update(name="vault-approle-api")),
        ("Deployment", FINISH, lambda w: volume(w, "vault-sources")["projected"]["sources"].insert(1, {"secret": {"name": "vault-approle-api", "items": [{"key": "role-id", "path": "api.role-id"}]}})),
        ("Deployment", FINISH, lambda w: volume(w, "vault-sources")["projected"].update(defaultMode=292)),
        ("Deployment", FINISH, lambda w: volume(w, "vault").update(emptyDir={})),
        ("Deployment", FINISH, lambda w: copier(w)["command"].__setitem__(2, "0640")),
        ("Deployment", FINISH, lambda w: main(w)["volumeMounts"].append({"name": "vault-sources", "mountPath": "/run/secrets/vault-connection-manager", "readOnly": True})),
        ("Deployment", FINISH, lambda w: pod_spec(w)["volumes"].append({"name": "kube", "projected": {"sources": [{"serviceAccountToken": {"path": "token"}}]}})),
        ("Deployment", FINISH, lambda w: w["spec"]["template"]["metadata"]["labels"].update({"mandate.dev/egress-kube-api": "true"})),
        ("NetworkPolicy", f"egress-{FINISH}", lambda p: vault_rule(p)["ports"].append({"protocol": "TCP", "port": 8201})),
        ("NetworkPolicy", f"egress-{FINISH}", lambda p: p["spec"]["egress"].append({"to": [{"ipBlock": {"cidr": "0.0.0.0/0"}}], "ports": [{"protocol": "TCP", "port": 443}]})),
        ("Deployment", "api", lambda w: volume(w, "vault-sources")["projected"]["sources"].insert(1, copy.deepcopy(manager))),
        ("Deployment", "api", lambda w: copier(w)["command"].insert(5, "/run/secrets/vault-api/connection-manager.role-id")),
        ("Deployment", "api", lambda w: pod_spec(w)["volumes"].append({"name": "leak", "secret": {"secretName": "vault-approle-connection-manager"}})),
        ("Deployment", "scheduler", lambda w: pod_spec(w).setdefault("volumes", []).append({"name": "leak", "secret": {"secretName": "vault-approle-connection-manager"}})),
        ("Deployment", PROVISIONER, lambda w: main(w).setdefault("envFrom", []).append({"secretRef": {"name": "vault-approle-connection-manager"}})),
    ]
    for kind, name, seed in seeds:
        assert connect_finish(mutate(on, name, kind, seed), on=True, workspace=workspace), (name, seed)
    base = ["-f", str(CELL / "values" / "k3d.yaml"), *every_image()]
    assert helm(CHART, *base, "--set", "api.connectionManagerBinding=true").returncode != 0, "the API's old binding is gone"


def beside_api(w: dict, where: str, volume: str) -> None:
    """A copy of the api container, hardened as it is, added to the api pod's `where` with only its
    mounts changed: `volume` read-only and nothing else."""
    spec = pod_spec(w)
    other = copy.deepcopy(next(c for c in spec["containers"] if c["name"] == "api"))
    other["name"] = "beside"
    other["volumeMounts"] = [{"name": volume, "mountPath": "/run/beside", "readOnly": True}]
    spec.setdefault(where, []).append(other)


def api_vault_address(labels: dict) -> str:
    """The vault's address in the api's workspace namespace, the executor's own."""
    return f"https://vault.ws-{labels.get('mandate.dev/workspace')}.svc:8200"


def api_vault_rule(labels: dict) -> dict:
    """The api's one egress rule to the vault: its workspace's vault pod, on 8200 alone."""
    return {
        "ports": [{"port": 8200, "protocol": "TCP"}],
        "to": [{"podSelector": {"matchLabels": {"mandate.dev/process": "vault", "mandate.dev/workspace": labels.get("mandate.dev/workspace")}}}],
    }


def api_egress_bound(on: list[dict], off: list[dict]) -> list[str]:
    """The api's whole egress with its binding: the rules it has without it, with the vault rule
    after the journal's and nothing else added, widened or dropped."""
    rules = policies(off)["egress-api"]["spec"]["egress"]
    api = next(w for w in workloads(on) if w["metadata"]["name"] == "api")
    wanted = rules[:2] + [api_vault_rule(api["spec"]["template"]["metadata"]["labels"])] + rules[2:]
    got = policies(on)["egress-api"]["spec"]["egress"]
    return [] if got == wanted else [f"the api's egress is {got}, not {wanted}"]


def test_the_api_is_bound_to_the_vault_only_with_its_binding():
    identity = ("--set-json", 'egress.identity-provider=["192.0.2.0/24"]')
    off = render_all(*identity)
    on = render_all(*identity, "--set", "api.vaultBinding=true")
    assert any(p["port"] == 443 for r in policies(on)["egress-api"]["spec"]["egress"] for p in r["ports"])
    assert api_vault(off, on=False) == []
    assert api_vault(on, on=True) == []
    assert api_egress_bound(on, off) == []
    for check in (hardened, gated, secrets_as_files, api_contained, default_deny):
        assert check(on) == [], check.__name__
    assert api_vault(on, on=False), "a binding while off is caught"
    assert api_vault(off, on=True), "no binding while on is caught"
    seeds = [
        ("Deployment", lambda w: next(v for v in pod_spec(w)["volumes"] if v["name"] == "vault-sources")["projected"].update(defaultMode=292)),
        ("Deployment", lambda w: next(v for v in pod_spec(w)["volumes"] if v["name"] == "vault-sources")["projected"]["sources"][0]["secret"].update(name="vault-approle-runtime")),
        ("Deployment", lambda w: next(v for v in pod_spec(w)["volumes"] if v["name"] == "vault-sources")["projected"]["sources"].append({"secret": {"name": "journal-ca"}})),
        ("Deployment", lambda w: next(v for v in pod_spec(w)["volumes"] if v["name"] == "vault").update(emptyDir={})),
        ("Deployment", lambda w: next(c for c in pod_spec(w)["initContainers"] if c["name"] == "vault-credentials")["command"].__setitem__(2, "0640")),
        ("Deployment", lambda w: next(c for c in pod_spec(w)["containers"] if c["name"] == "api")["volumeMounts"].append({"name": "vault-sources", "mountPath": "/run/secrets/vault-api", "readOnly": True})),
        ("Deployment", lambda w: next(m for m in next(c for c in pod_spec(w)["containers"] if c["name"] == "api")["volumeMounts"] if m["mountPath"] == API_VAULT_DIR).update(readOnly=False)),
        ("Deployment", lambda w: next(c for c in pod_spec(w)["containers"] if c["name"] == "api")["env"].append({"name": "MANDATE_API_VAULT_SECRET_ID", "value": "x"})),
        ("Deployment", lambda w: w["spec"]["template"]["metadata"]["labels"].pop("mandate.dev/egress-vault")),
        ("NetworkPolicy", lambda p: vault_rule(p)["ports"].append({"protocol": "TCP", "port": 8201})),
        ("NetworkPolicy", lambda p: vault_rule(p)["to"].append({"ipBlock": {"cidr": "0.0.0.0/0"}})),
        ("NetworkPolicy", lambda p: vault_rule(p)["to"][0].update(namespaceSelector={})),
        ("NetworkPolicy", lambda p: p["spec"]["egress"].append({"to": [{"ipBlock": {"cidr": "0.0.0.0/0"}}], "ports": [{"protocol": "TCP", "port": 8200}]})),
        ("NetworkPolicy", lambda p: p["spec"]["egress"].append({"to": [{"podSelector": {}}]})),
        ("Deployment", lambda w: next(c for c in pod_spec(w)["containers"] if c["name"] == "api")["env"].__setitem__(
            next(i for i, e in enumerate(next(c for c in pod_spec(w)["containers"] if c["name"] == "api")["env"]) if e["name"] == "MANDATE_API_VAULT_ADDRESS"),
            {"name": "MANDATE_API_VAULT_ADDRESS", "value": "https://vault.example.com:8200"})),
    ]
    for kind, seed in seeds:
        name = "api" if kind == "Deployment" else "egress-api"
        assert api_vault(mutate(on, name, kind, seed), on=True), seed
    for where, volume in (("containers", "vault"), ("containers", "vault-sources"), ("initContainers", "vault-sources")):
        assert api_vault(mutate(on, "api", "Deployment", lambda w: beside_api(w, where, volume)), on=True), (where, volume)
    off_volumes = lambda w: (pod_spec(w).setdefault("volumes", []).append({"name": "vault", "emptyDir": {"medium": "Memory"}}), beside_api(w, "containers", "vault"))
    assert api_vault(mutate(off, "api", "Deployment", off_volumes), on=False), "a vault volume mounted while off is caught"
    widened = [
        lambda p: next(r for r in p["spec"]["egress"] if any(q["port"] == 443 for q in r["ports"]))["to"].append({"podSelector": {}}),
        lambda p: p["spec"]["egress"].pop(),
        lambda p: p["spec"]["egress"][1]["to"][0].update(namespaceSelector={}),
    ]
    for seed in widened:
        assert api_egress_bound(mutate(on, "egress-api", "NetworkPolicy", seed), off), seed
    leaks = [
        ("Deployment", other, lambda w: pod_spec(w).setdefault("volumes", []).append({"name": "leak", "secret": {"secretName": "vault-approle-api"}}))
        for other in ("deploy-manager", "scheduler", "model-gateway", "exporter", "data")
    ] + [
        ("Deployment", "scheduler", lambda w: pod_spec(w)["containers"][0].setdefault("envFrom", []).append({"secretRef": {"name": "vault-approle-api"}})),
        ("Deployment", "data", lambda w: containers(w)[0].setdefault("env", []).append({"name": "X", "valueFrom": {"secretKeyRef": {"name": "vault-approle-api", "key": "secret-id"}}})),
        ("Deployment", "exporter", lambda w: pod_spec(w).setdefault("initContainers", []).append({"name": "leak", "image": "x", "args": ["vault-approle-api"]})),
        ("Pod", "executor", lambda w: pod_spec(w).setdefault("volumes", []).append({"name": "leak", "secret": {"secretName": "vault-approle-api"}})),
        ("Deployment", "api", lambda w: pod_spec(w)["volumes"].append({"name": "again", "secret": {"secretName": "vault-approle-api"}})),
    ]
    for kind, name, seed in leaks:
        assert api_vault(mutate(on, name, kind, seed), on=True), (name, seed)
    assert api_vault(mutate(off, "scheduler", "Deployment", leaks[1][2]), on=False), "a leak while off is caught"


APPROLE_SECRET = "vault-approle-"
APPROLE_COPY_MODES = (["install", "-m", "0600"], ["install", "-m", "0400"])
POD_KINDS = {"Deployment", "Job", "CronJob", "StatefulSet", "DaemonSet", "ReplicaSet", "ReplicationController"}


def other_pods(docs: list[dict]) -> list[dict]:
    """The pod templates of every kind but the workspace's own (Deployments, Jobs, managed pods),
    each shaped as a workload: a bare Pod's own spec, a CronJob's job template's, and the rest's own."""
    shaped = {
        "Pod": lambda d: {"template": {"metadata": d["metadata"], "spec": d["spec"]}},
        "CronJob": lambda d: d["spec"]["jobTemplate"]["spec"],
    }
    return [
        {"kind": d["kind"], "metadata": d["metadata"], "spec": shaped.get(d["kind"], lambda d: d["spec"])(d)}
        for d in docs
        if d["kind"] in POD_KINDS - {"Deployment", "Job"} or d["kind"] == "Pod"
    ]


def approle_copied(docs: list[dict]) -> list[str]:
    """No process reads a vault AppRole Secret (`vault-approle-*`, DEC-2630) where it is mounted:
    under the pods' fsGroup the kubelet makes each file 0440, which `read_mounted_credentials`
    refuses, as it must for a secret. A volume that names one is mounted only by an init container
    that runs to completion and `install`s its files owner-only into a memory-backed directory, the
    pgpass pattern; no container, sidecar included, mounts it or names it in its environment,
    command or arguments."""
    problems = [
        f"{d['kind']} {d['metadata']['name']} runs pods this check does not read"
        for d in docs
        if d["kind"] not in POD_KINDS and ("template" in d.get("spec", {}) or "jobTemplate" in d.get("spec", {}))
    ]
    for w in workloads(docs) + other_pods(docs):
        spec = pod_spec(w)
        name = w["metadata"]["name"]
        volumes = spec.get("volumes", [])
        held = {v["name"] for v in volumes if APPROLE_SECRET in json.dumps(v)}
        memory = {v["name"] for v in volumes if (v.get("emptyDir") or {}).get("medium") == "Memory"}
        copies = [c for c in spec.get("initContainers", []) if c.get("restartPolicy") != "Always"]
        readers = spec.get("containers", []) + [c for c in spec.get("initContainers", []) if c not in copies]
        for c in readers + copies:
            named = json.dumps({k: c.get(k) for k in ("env", "envFrom", "command", "args")})
            if APPROLE_SECRET in named:
                problems.append(f"{name}/{c['name']} names an AppRole Secret in its environment or command")
        for c in readers:
            if held & {m["name"] for m in c.get("volumeMounts", [])}:
                problems.append(f"{name}/{c['name']} mounts an AppRole Secret it would read at 0440")
        for c in copies:
            mounts = c.get("volumeMounts", [])
            if not held & {m["name"] for m in mounts}:
                continue
            into = {m["name"] for m in mounts if not m.get("readOnly")}
            if c.get("command", [])[:3] not in APPROLE_COPY_MODES or c.get("args"):
                problems.append(f"{name}/{c['name']} copies an AppRole Secret by {c.get('command')} {c.get('args')}")
            if not into or not into <= memory or into & held:
                problems.append(f"{name}/{c['name']} copies an AppRole Secret into {sorted(into)}, not memory alone")
            targets = {m["mountPath"].rstrip("/") for m in mounts if not m.get("readOnly") and m["name"] in memory}
            if not c.get("command") or c["command"][-1].rstrip("/") not in targets:
                problems.append(f"{name}/{c['name']} installs an AppRole Secret's files into {c.get('command', [None])[-1:]}, not its memory mount")
            if any(m["name"] in held and not m.get("readOnly") for m in mounts):
                problems.append(f"{name}/{c['name']} mounts an AppRole Secret writable")
        everyone = spec.get("containers", []) + spec.get("initContainers", [])
        for volume in held:
            by = [c["name"] for c in everyone if volume in {m["name"] for m in c.get("volumeMounts", [])}]
            if len(by) > 1:
                problems.append(f"{name}: the AppRole volume {volume} is mounted by {by}, not its one copy")
        for c in copies:
            if not held & {m["name"] for m in c.get("volumeMounts", [])}:
                continue
            for target in {m["name"] for m in c.get("volumeMounts", []) if not m.get("readOnly")}:
                others = [(o["name"], m.get("readOnly", False), o in copies) for o in everyone if o is not c for m in o.get("volumeMounts", []) if m["name"] == target]
                if len(others) > 1 or any(not read_only or init for _, read_only, init in others):
                    problems.append(f"{name}: the copied credentials in {target} are mounted by {others}, not one reader read-only")
    return problems


def test_no_process_reads_an_approle_secret_where_it_is_mounted():
    for docs in (render_all(), render_all("--set", "api.vaultBinding=true")):
        assert approle_copied(docs) == []
    on = render_all("--set", "api.vaultBinding=true")
    copier = lambda w: next(c for c in pod_spec(w)["initContainers"] if c["name"] == "vault-credentials")
    leak = {"name": "leak", "secret": {"secretName": "vault-approle-model-gateway", "defaultMode": 256}}
    seeds = [
        ("Deployment", "api", lambda w: next(c for c in pod_spec(w)["containers"] if c["name"] == "api")["volumeMounts"].append({"name": "vault-sources", "mountPath": "/run/secrets/vault-api", "readOnly": True})),
        ("Deployment", "api", lambda w: copier(w).update(restartPolicy="Always")),
        ("Deployment", "api", lambda w: copier(w).update(command=["cp", "-r", "/run/secrets/vault-api/.", API_VAULT_DIR])),
        ("Deployment", "api", lambda w: copier(w)["command"].__setitem__(2, "0640")),
        ("Deployment", "api", lambda w: next(v for v in pod_spec(w)["volumes"] if v["name"] == "vault")["emptyDir"].pop("medium")),
        ("Deployment", "api", lambda w: next(m for m in copier(w)["volumeMounts"] if m["name"] == "vault-sources").update(readOnly=False)),
        ("Deployment", "model-gateway", lambda w: (pod_spec(w).setdefault("volumes", []).append(leak), pod_spec(w)["containers"][0].setdefault("volumeMounts", []).append({"name": "leak", "mountPath": "/run/mandate/vault", "readOnly": True}))),
        ("Deployment", "scheduler", lambda w: pod_spec(w)["containers"][0].setdefault("envFrom", []).append({"secretRef": {"name": "vault-approle-runtime"}})),
        ("Pod", "runtime", lambda w: (pod_spec(w).setdefault("volumes", []).append(leak), pod_spec(w)["containers"][0].setdefault("volumeMounts", []).append({"name": "leak", "mountPath": "/run/mandate/vault", "readOnly": True}))),
    ]
    for kind, name, seed in seeds:
        assert approle_copied(mutate(on, name, kind, seed)), (name, seed)
    named = [
        lambda w: copier(w).setdefault("env", []).append({"name": "X", "valueFrom": {"secretKeyRef": {"name": "vault-approle-api", "key": "secret-id"}}}),
        lambda w: copier(w).setdefault("envFrom", []).append({"secretRef": {"name": "vault-approle-api"}}),
        lambda w: copier(w)["command"].append("vault-approle-api"),
        lambda w: copier(w)["command"].__setitem__(-1, "/etc/"),
    ]
    for seed in named:
        assert approle_copied(mutate(on, "api", "Deployment", seed)), seed
    assert {d["kind"] for d in on if d["kind"] in POD_KINDS} == {"Deployment", "Job"}, "the workspace runs pods as Deployments and Jobs alone"
    leaked = next(d for d in on if d["kind"] == "Deployment" and d["metadata"]["name"] == "scheduler")
    leaked = copy.deepcopy(leaked)
    pod_spec(leaked).setdefault("volumes", []).append(leak)
    pod_spec(leaked)["containers"][0].setdefault("volumeMounts", []).append({"name": "leak", "mountPath": "/run/leak", "readOnly": True})
    for kind in ("StatefulSet", "DaemonSet", "ReplicaSet", "CronJob", "Rollout"):
        escaped = copy.deepcopy(leaked)
        escaped["kind"] = kind
        if kind == "CronJob":
            escaped["spec"] = {"jobTemplate": {"spec": escaped["spec"]}}
        assert approle_copied([d for d in on if d["metadata"]["name"] != "scheduler"] + [escaped]), kind
    bare = {"kind": "Pod", "metadata": {"name": "bare"}, "spec": copy.deepcopy(pod_spec(leaked))}
    assert approle_copied(on + [bare]), "a bare Pod is read"
    assert not any(d["kind"] == "Pod" for d in on), "the workspace runs no bare Pod"
    for where, volume in (("containers", "vault"), ("containers", "vault-sources"), ("initContainers", "vault-sources"), ("initContainers", "vault")):
        assert approle_copied(mutate(on, "api", "Deployment", lambda w: beside_api(w, where, volume))), (where, volume)


def vault_rule(policy: dict) -> dict:
    """The rule of an egress policy that names the vault's pods."""
    return next(r for r in policy["spec"]["egress"] if any(t.get("podSelector", {}).get("matchLabels", {}).get("mandate.dev/process") == "vault" for t in r["to"]))


def api_env(rendered: list[dict]) -> dict[str, str]:
    """The api container's environment, name to value."""
    return {e["name"]: e.get("value", "") for e in first_container(owner_workload(rendered)).get("env", [])}


PROVISIONER = "role-provisioner"
PROVISIONER_VAULT_SOURCES = {
    "defaultMode": 288,
    "sources": [
        {
            "secret": {
                "name": "vault-approle-provisioner",
                "items": [
                    {"key": "role-id", "path": "provisioner.role-id"},
                    {"key": "secret-id", "path": "provisioner.secret-id"},
                ],
            }
        },
        {"configMap": {"name": "vault-ca", "items": [{"key": "ca.crt", "path": "ca.crt"}]}},
    ],
}
PROVISIONER_VAULT_COPY = [
    "install",
    "-m",
    "0600",
    "/run/secrets/vault-provisioner/provisioner.role-id",
    "/run/secrets/vault-provisioner/provisioner.secret-id",
    "/run/secrets/vault-provisioner/ca.crt",
    f"{API_VAULT_DIR}/",
]
PROVISIONER_ENV = {"MANDATE_JOURNAL_DSN", "PGPASSFILE", "HOME", "MANDATE_DEPLOYMENT_TIER"}
# Every volume of the provisioner's pod, by name and kind: its scratch space, its journal role's
# pgpass copy and Secret and the journal's CA, and its vault copy and sources. Anything else, a
# projected service-account token or another ConfigMap among them, is a credential it must not get.
PROVISIONER_VOLUMES = {
    "tmp": "emptyDir",
    "work": "emptyDir",
    "pg": "emptyDir",
    "journal-role": "secret",
    "journal-ca": "secret",
    "vault": "emptyDir",
    "vault-sources": "projected",
}


def provisioner_contained(docs: list[dict], workspace: str) -> list[str]:
    """The role provisioner's pod (DEC-2350, DEC-2630 item 7) holds its own AppRole and the vault's
    CA alone, as #2227's owner-only copy makes them, and no Kubernetes credential: no service-account
    token, no account of its own, no Role or binding; it reads the control stream of the hosted
    workspace `workspace` as its own journal role, and binds roles in its own namespace; and its
    egress is the journal and its workspace's vault on 8200, nothing else."""
    problems = []
    found = [w for w in workloads(docs) if w["metadata"]["name"] == PROVISIONER]
    if [w["kind"] for w in found] != ["Deployment"]:
        return [f"the role provisioner renders as {[w['kind'] for w in found]}"]
    w = found[0]
    spec = pod_spec(w)
    labels = w["spec"]["template"]["metadata"]["labels"]
    if spec.get("automountServiceAccountToken") is not False or spec.get("serviceAccountName", "default") != "default" or spec.get("serviceAccount", "default") != "default":
        problems.append("the role provisioner's pod holds a service-account token or an account of its own")
    if any(d["kind"] in ("ServiceAccount", "Role", "RoleBinding") and PROVISIONER in json.dumps(d) for d in docs):
        problems.append("a ServiceAccount, Role or binding names the role provisioner")
    main = next((c for c in spec["containers"] if c["name"] == PROVISIONER), None)
    if main is None or len(spec["containers"]) != 1:
        return problems + [f"the role provisioner's containers are {[c['name'] for c in spec['containers']]}"]
    namespace = api_vault_address(labels).split(".")[1]
    args = [
        "run",
        "--workspace",
        workspace,
        "--namespace",
        namespace,
        "--probe",
        "/tmp/process.sock",
        "--vault-address",
        api_vault_address(labels),
        "--vault-dir",
        API_VAULT_DIR,
    ]
    if main.get("args") != args or main.get("command"):
        problems.append(f"the role provisioner starts as {main.get('command')} {main.get('args')}")
    env = {e["name"]: e for e in main.get("env", [])}
    if set(env) != PROVISIONER_ENV or any("valueFrom" in e for e in env.values()) or main.get("envFrom"):
        problems.append(f"the role provisioner's environment is {sorted(env)}")
    if not env.get("MANDATE_JOURNAL_DSN", {}).get("value", "").startswith("postgres://mandate_role_provisioner@"):
        problems.append("the role provisioner reaches the journal as another role")
    volumes = {v["name"]: v for v in spec.get("volumes", [])}
    kinds = {n: sorted(k for k in v if k != "name") for n, v in volumes.items()}
    if kinds != {n: [k] for n, k in PROVISIONER_VOLUMES.items()} or len(volumes) != len(spec.get("volumes", [])):
        problems.append(f"the role provisioner's pod has the volumes {kinds}")
    sources = [k for v in volumes.values() if "projected" in v for x in v["projected"]["sources"] for k in x]
    if sorted(sources) != ["configMap", "secret"]:
        problems.append(f"the role provisioner's projected sources are {sources}")
    secrets = sorted(
        [v["secret"]["secretName"] for v in volumes.values() if "secret" in v]
        + [x["secret"]["name"] for v in volumes.values() if "projected" in v for x in v["projected"]["sources"] if "secret" in x]
    )
    if secrets != sorted(["journal-ca", "journal-role-mandate-role-provisioner", "vault-approle-provisioner"]):
        problems.append(f"the role provisioner's pod mounts the Secrets {secrets}")
    if volumes.get("vault-sources", {}).get("projected") != PROVISIONER_VAULT_SOURCES:
        problems.append(f"the role provisioner's vault sources are {volumes.get('vault-sources')}")
    if volumes.get("vault", {}).get("emptyDir", {}).get("medium") != "Memory":
        problems.append(f"the role provisioner's vault directory is {volumes.get('vault')}")
    copies = [c for c in spec.get("initContainers", []) if c["name"] == "vault-credentials"]
    if [c.get("command") for c in copies] != [PROVISIONER_VAULT_COPY]:
        problems.append(f"the role provisioner's credentials are copied by {[c.get('command') for c in copies]}")
    holders = sorted(
        (m["name"], c["name"], m.get("mountPath"), m.get("readOnly", False))
        for c in spec.get("containers", []) + spec.get("initContainers", [])
        for m in c.get("volumeMounts", [])
        if m["name"] in ("vault", "vault-sources")
    )
    wanted = [
        ("vault", PROVISIONER, API_VAULT_DIR, True),
        ("vault", "vault-credentials", API_VAULT_DIR, False),
        ("vault-sources", "vault-credentials", "/run/secrets/vault-provisioner", True),
    ]
    if holders != wanted:
        problems.append(f"the role provisioner's vault volumes are mounted by {holders}")
    egress = sorted(k for k, v in labels.items() if k.startswith("mandate.dev/egress-") and v == "true")
    if egress != ["mandate.dev/egress-postgres", "mandate.dev/egress-vault"]:
        problems.append(f"the role provisioner's pod carries the egress labels {egress}")
    rules = policies(docs)[f"egress-{PROVISIONER}"]["spec"]["egress"]
    if any("to" not in r or "ports" not in r for r in rules):
        problems.append(f"a role provisioner egress rule names no peer or no port: {rules}")
    reaching = [r for r in rules if '"vault"' in json.dumps(r) or any(p.get("port") == 8200 for p in r.get("ports", []))]
    if reaching != [api_vault_rule(labels)]:
        problems.append(f"the role provisioner's egress to the vault is {reaching}")
    if len(rules) != 3:
        problems.append(f"the role provisioner has {len(rules)} egress rules, not DNS, the journal and the vault")
    holders = [
        (d["kind"], d["metadata"]["name"])
        for d in workloads(docs)
        for _ in range(json.dumps(d).count('"vault-approle-provisioner"'))
    ]
    if holders != [("Deployment", PROVISIONER)]:
        problems.append(f"the role provisioner's AppRole is named by {holders}")
    return problems


def test_the_role_provisioner_holds_its_approle_alone_and_no_kubernetes_credential():
    docs = render_all()
    workspace = next(
        d for d in docs if d["kind"] == "Deployment" and d["metadata"]["name"] == MANAGER
    )["spec"]["template"]["spec"]["containers"][0]["args"][2]
    assert provisioner_contained(docs, workspace) == []
    for check in (hardened, gated, secrets_as_files, default_deny, egress_exact):
        assert check(docs) == [], check.__name__
    main = lambda w: next(c for c in pod_spec(w)["containers"] if c["name"] == PROVISIONER)
    copy_of = lambda w: next(c for c in pod_spec(w)["initContainers"] if c["name"] == "vault-credentials")
    volume = lambda w, n: next(v for v in pod_spec(w)["volumes"] if v["name"] == n)
    seeds = [
        ("Deployment", lambda w: pod_spec(w).update(automountServiceAccountToken=True)),
        ("Deployment", lambda w: pod_spec(w).update(serviceAccountName=MANAGER)),
        ("Deployment", lambda w: volume(w, "vault-sources")["projected"].update(defaultMode=292)),
        ("Deployment", lambda w: volume(w, "vault-sources")["projected"]["sources"][0]["secret"].update(name="vault-approle-api")),
        ("Deployment", lambda w: volume(w, "vault-sources")["projected"]["sources"].append({"secret": {"name": "vault-approle-executor-6331"}})),
        ("Deployment", lambda w: pod_spec(w)["volumes"].append({"name": "leak", "secret": {"secretName": "vault-approle-connection-manager"}})),
        ("Deployment", lambda w: pod_spec(w)["volumes"].append({"name": "kube", "projected": {"sources": [{"serviceAccountToken": {"path": "token", "expirationSeconds": 600}}]}})),
        ("Deployment", lambda w: volume(w, "vault-sources")["projected"]["sources"].append({"serviceAccountToken": {"path": "token"}})),
        ("Deployment", lambda w: pod_spec(w)["volumes"].append({"name": "extra", "configMap": {"name": "deploy-manager-templates"}})),
        ("Deployment", lambda w: pod_spec(w)["volumes"].append({"name": "host", "hostPath": {"path": "/var/run"}})),
        ("Deployment", lambda w: pod_spec(w)["volumes"].append({"name": "down", "downwardAPI": {"items": [{"path": "ns", "fieldRef": {"fieldPath": "metadata.namespace"}}]}})),
        ("Deployment", lambda w: volume(w, "work").update(configMap={"name": "vault-ca"})),
        ("Deployment", lambda w: volume(w, "vault").update(emptyDir={})),
        ("Deployment", lambda w: copy_of(w)["command"].__setitem__(2, "0640")),
        ("Deployment", lambda w: main(w)["volumeMounts"].append({"name": "vault-sources", "mountPath": "/run/secrets/vault-provisioner", "readOnly": True})),
        ("Deployment", lambda w: next(m for m in main(w)["volumeMounts"] if m["name"] == "vault").update(readOnly=False)),
        ("Deployment", lambda w: main(w)["env"].append({"name": "MANDATE_VAULT_SECRET_ID", "value": "x"})),
        ("Deployment", lambda w: main(w)["env"].append({"name": "X", "valueFrom": {"secretKeyRef": {"name": "vault-approle-provisioner", "key": "secret-id"}}})),
        ("Deployment", lambda w: main(w)["args"].__setitem__(4, "kube-system")),
        ("Deployment", lambda w: main(w)["args"].__setitem__(2, "ws9")),
        ("Deployment", lambda w: main(w)["args"].__setitem__(8, "https://vault.example.com:8200")),
        ("Deployment", lambda w: main(w)["args"].append("--fixture")),
        ("Deployment", lambda w: main(w)["env"][0].update(value="postgres://mandate_deploy_manager@journal-rw:5432/mandate")),
        ("Deployment", lambda w: w["spec"]["template"]["metadata"]["labels"].update({"mandate.dev/egress-kube-api": "true"})),
        ("Deployment", lambda w: w["spec"]["template"]["metadata"]["labels"].pop("mandate.dev/egress-vault")),
        ("NetworkPolicy", lambda p: vault_rule(p)["ports"].append({"protocol": "TCP", "port": 8201})),
        ("NetworkPolicy", lambda p: vault_rule(p)["to"][0].update(namespaceSelector={})),
        ("NetworkPolicy", lambda p: p["spec"]["egress"].append({"to": [{"ipBlock": {"cidr": "0.0.0.0/0"}}], "ports": [{"protocol": "TCP", "port": 443}]})),
        ("NetworkPolicy", lambda p: p["spec"]["egress"].append({"to": [{"podSelector": {}}]})),
    ]
    for kind, seed in seeds:
        name = PROVISIONER if kind == "Deployment" else f"egress-{PROVISIONER}"
        assert provisioner_contained(mutate(docs, name, kind, seed), workspace), seed
    leaks = [
        ("Deployment", "api", lambda w: pod_spec(w).setdefault("volumes", []).append({"name": "leak", "secret": {"secretName": "vault-approle-provisioner"}})),
        ("Deployment", MANAGER, lambda w: containers(w)[0].setdefault("envFrom", []).append({"secretRef": {"name": "vault-approle-provisioner"}})),
        ("Pod", "executor", lambda w: pod_spec(w).setdefault("volumes", []).append({"name": "leak", "secret": {"secretName": "vault-approle-provisioner"}})),
    ]
    for kind, name, seed in leaks:
        assert provisioner_contained(mutate(docs, name, kind, seed), workspace), (kind, name)
    granted = copy.deepcopy(docs)
    granted.append({"kind": "RoleBinding", "metadata": {"name": "x"}, "subjects": [{"kind": "ServiceAccount", "name": PROVISIONER}]})
    assert provisioner_contained(granted, workspace), "a binding that names the provisioner is caught"


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


# The public CA bundle a process trusts for the hosts it reaches over public TLS (RS's finding 1,
# DEC-2371): copied from the pinned postgres image by an init container of its pod, since
# every process image is a bare binary with no roots. The API trusts it for the identity provider's
# key set; the token exchange for Alpaca's token host, which without it fails every connect
# `endpoint_failed`; the executor for the broker; and so on for each public destination below.
CA_BUNDLE_DIR = "/run/ca-bundle"
CA_BUNDLE = f"{CA_BUNDLE_DIR}/ca-certificates.crt"
SYSTEM_BUNDLE = "/etc/ssl/certs/ca-certificates.crt"
# The egress destinations reached over public TLS, written out here rather than read from the chart.
# The vault, Postgres, the API server and the cell's own services are each trusted by their own CA.
PUBLIC_TLS = {
    "identity-provider",
    "broker",
    "market-data",
    "token-host",
    "model-providers",
    "object-storage",
    "timestamping",
}


def wants_ca_bundle(process_entry: dict) -> bool:
    return process_entry["name"] == "api" or bool(PUBLIC_TLS & set(process_entry.get("egress", [])))


def ca_bundle(docs: list[dict]) -> list[str]:
    """Each process container whose egress names a public TLS host, the API's included, mounts the
    CA bundle read-only and names it in SSL_CERT_FILE, and one init container of its pod copies it
    from the system bundle; no other container, a sidecar included, gets it."""
    problems = []
    for w in workloads(docs):
        spec = pod_spec(w)
        wants = wants_ca_bundle(owner(w))
        for index, c in enumerate(spec.get("containers", [])):
            env = {e["name"]: e.get("value") for e in c.get("env", [])}
            mounts = {m["mountPath"]: m for m in c.get("volumeMounts", [])}
            name = f"{w['metadata']['name']}/{c['name']}"
            if not (wants and index == 0):
                if "SSL_CERT_FILE" in env or CA_BUNDLE_DIR in mounts:
                    problems.append(f"{name} gets the CA bundle")
                continue
            mount = mounts.get(CA_BUNDLE_DIR)
            if env.get("SSL_CERT_FILE") != CA_BUNDLE:
                problems.append(f"{name}'s SSL_CERT_FILE is {env.get('SSL_CERT_FILE')!r}")
            if not mount or mount.get("readOnly") is not True:
                problems.append(f"{name} does not mount the CA bundle read-only: {mount}")
                continue
            volume = next((v for v in spec.get("volumes", []) if v.get("name") == mount["name"]), {})
            if volume.get("emptyDir") != {"sizeLimit": "1Mi"}:
                problems.append(f"{name}'s CA bundle is not a 1Mi emptyDir: {volume}")
            copiers = [
                i
                for i in spec.get("initContainers", [])
                if any(m["name"] == mount["name"] for m in i.get("volumeMounts", []))
                and f"cp {SYSTEM_BUNDLE} {CA_BUNDLE}" in " ".join(i.get("command", []))
            ]
            if len(copiers) != 1:
                problems.append(f"{name}'s CA bundle is not copied by one init container")
    return problems


def test_each_public_tls_process_gets_the_ca_bundle_and_no_other():
    docs = render_all()
    assert ca_bundle(docs) == []
    given = {
        w["metadata"]["name"]
        for w in workloads(docs)
        if CA_BUNDLE_DIR in {m["mountPath"] for m in first_container(w).get("volumeMounts", [])}
    }
    assert {"api", "token-exchange", "executor"} <= given, given
    assert not given & {"migrate", "deploy-manager", "role-provisioner", "runtime"}, given
    api = owner_workload(docs)
    assert CA_BUNDLE_DIR in {m["mountPath"] for m in first_container(api)["volumeMounts"]}
    for name, kind in (("api", "Deployment"), ("token-exchange", "Pod"), ("executor", "Pod")):
        for seed in (
            lambda w: next(
                m for m in first_container(w)["volumeMounts"] if m["mountPath"] == CA_BUNDLE_DIR
            ).update(readOnly=False),
            lambda w: first_container(w).update(
                env=[e for e in first_container(w)["env"] if e["name"] != "SSL_CERT_FILE"]
            ),
            lambda w: pod_spec(w).update(
                initContainers=[
                    i
                    for i in pod_spec(w).get("initContainers", [])
                    if SYSTEM_BUNDLE not in " ".join(i.get("command", []))
                ]
            ),
            lambda w: next(v for v in pod_spec(w)["volumes"] if v["name"] == "ca-bundle").update(emptyDir={}),
            lambda w: next(v for v in pod_spec(w)["volumes"] if v["name"] == "ca-bundle").update(
                emptyDir={"sizeLimit": "1Gi"}
            ),
        ):
            assert ca_bundle(mutate(docs, name, kind, seed)), (name, seed)
    leaked = mutate(
        docs,
        "role-provisioner",
        "Deployment",
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


RESEARCH_MANAGER = "deploy-manager-research"
RESEARCH_WORKSPACE = "01KDEM0000000000000000000Wr"


def two_managers(*extra: str) -> list[dict]:
    """Every process rendered, with the local cell's second manager for the research workspace."""
    return render_all("--set", f"deployManager.researchWorkspace={RESEARCH_WORKSPACE}", *extra)


def test_the_second_manager_is_bound_as_the_first_for_its_own_workspace():
    """DEC-2070 items 1 and 3: the second manager runs the first's image and launch for the research
    workspace alone, under its own ServiceAccount, Role and binding with exactly the first's rules,
    and its own admission and accounts policies, each matching only its own account, with exactly
    the first's validations; so neither manager can make a pod or an account no policy checks."""
    docs = two_managers()
    managers = {w["metadata"]["name"]: w for w in docs if w["kind"] == "Deployment" and w["metadata"]["name"] in (MANAGER, RESEARCH_MANAGER)}
    assert set(managers) == {MANAGER, RESEARCH_MANAGER}
    first, second = (first_container(managers[n]) for n in (MANAGER, RESEARCH_MANAGER))
    assert first["image"] == second["image"]
    assert second["args"][second["args"].index("--workspace") + 1] == RESEARCH_WORKSPACE
    assert first["args"][first["args"].index("--workspace") + 1] != RESEARCH_WORKSPACE
    assert pod_spec(managers[RESEARCH_MANAGER])["serviceAccountName"] == RESEARCH_MANAGER

    def by(kind: str) -> dict[str, dict]:
        return {d["metadata"]["name"]: d for d in docs if d["kind"] == kind}

    roles, bindings = by("Role"), by("RoleBinding")
    assert set(roles) == {MANAGER, RESEARCH_MANAGER}
    assert roles[MANAGER]["rules"] == roles[RESEARCH_MANAGER]["rules"]
    for name in (MANAGER, RESEARCH_MANAGER):
        assert bindings[name]["subjects"] == [{"kind": "ServiceAccount", "name": name, "namespace": "ws-ws1"}]
        assert bindings[name]["roleRef"] == {"apiGroup": "rbac.authorization.k8s.io", "kind": "Role", "name": name}
    for kind in ("Deployment", "ServiceAccount", "Role", "RoleBinding"):
        named = [d["metadata"]["name"] for d in docs if d["kind"] == kind and d["metadata"]["name"] == RESEARCH_MANAGER]
        assert named == [RESEARCH_MANAGER], (kind, named)
    egress = {
        n: [d for d in docs if d["kind"] == "NetworkPolicy" and d["metadata"]["name"] == f"egress-{n}"]
        for n in (MANAGER, RESEARCH_MANAGER)
    }
    assert [len(e) for e in egress.values()] == [1, 1], egress
    assert egress[RESEARCH_MANAGER][0]["spec"]["podSelector"]["matchLabels"]["mandate.dev/process"] == RESEARCH_MANAGER
    unselected = lambda e: {k: v for k, v in e[0]["spec"].items() if k != "podSelector"}
    assert unselected(egress[MANAGER]) == unselected(egress[RESEARCH_MANAGER])
    assert not [d for d in docs if d["kind"] == "Service" and RESEARCH_MANAGER in d["metadata"]["name"]]
    accounts = by("ServiceAccount")
    assert accounts[RESEARCH_MANAGER].get("automountServiceAccountToken") == accounts[MANAGER].get(
        "automountServiceAccountToken"
    )
    pods = {n: pod_spec(managers[n]) for n in (MANAGER, RESEARCH_MANAGER)}
    assert [v for v in pods[MANAGER]["volumes"] if v.get("persistentVolumeClaim")] == [
        v for v in pods[RESEARCH_MANAGER]["volumes"] if v.get("persistentVolumeClaim")
    ]
    assert [m for m in first["volumeMounts"] if m["name"] == "artifacts"] == [
        m for m in second["volumeMounts"] if m["name"] == "artifacts"
    ]
    policies_ = by("ValidatingAdmissionPolicy")
    policy_bindings = by("ValidatingAdmissionPolicyBinding")
    for suffix in ("pods", "accounts"):
        mine = {n: p for n, p in policies_.items() if n.endswith(f"-{suffix}")}
        names = sorted(mine)
        assert len(names) == 2, names
        account = {
            n: [c["expression"] for c in p["spec"]["matchConditions"]]
            for n, p in mine.items()
        }
        first_name = next(n for n in names if n.endswith(f"-{MANAGER}-{suffix}"))
        second_name = next(n for n in names if n.endswith(f"-{RESEARCH_MANAGER}-{suffix}"))
        assert account[first_name] == [f"request.userInfo.username == 'system:serviceaccount:ws-ws1:{MANAGER}'"]
        assert account[second_name] == [f"request.userInfo.username == 'system:serviceaccount:ws-ws1:{RESEARCH_MANAGER}'"]
        strip = lambda p: {k: v for k, v in p["spec"].items() if k != "matchConditions"}
        assert strip(mine[first_name]) == strip(mine[second_name]), suffix
        for name in names:
            assert policy_bindings[name]["spec"]["policyName"] == name
            assert policy_bindings[name]["spec"]["validationActions"] == ["Deny"]
    first_templates, second_templates = (
        next(d for d in docs if d["kind"] == "ConfigMap" and d["metadata"]["name"] == f"{n}-templates")
        for n in (MANAGER, RESEARCH_MANAGER)
    )
    assert set(first_templates["data"]) == set(second_templates["data"]) == {"executor.json", "runtime.json", f"{EXCHANGE}.json"}


@pytest.mark.parametrize("tier", ["dev", "ci", "staging", "production"])
def test_the_second_manager_runs_only_in_a_dev_or_ci_cell(tier: str):
    """DEC-2070 item 4: the chart renders the second manager in a dev or ci cell and refuses it at
    any other tier."""
    done = helm(
        CHART, "-f", str(CELL / "values" / "k3d.yaml"), *every_image(),
        "--set", f"deploymentTier={tier}", "--set", f"deployManager.researchWorkspace={RESEARCH_WORKSPACE}",
    )
    if tier in ("dev", "ci"):
        assert done.returncode == 0, done.stderr
    else:
        assert done.returncode != 0 and "runs only in a dev or ci cell" in done.stderr, done.stderr


@pytest.mark.parametrize(
    "named, hosted",
    [
        ("01KDEM0000000000000000000W", ""),
        ("01KDEM0000000000000000000Ws", ""),
        ("01KDEM0000000000000000000Xr", "01KDEM0000000000000000000W"),
    ],
)
def test_the_second_manager_serves_only_the_hosted_workspace_with_r(named: str, hosted: str):
    """DEC-2070 item 1: the second manager's workspace is the hosted workspace's ULID with `r`, so it
    never reads another manager's control stream; the schema refuses a bare ULID, and the chart a
    research workspace that is not the hosted one's."""
    done = helm(
        CHART, "-f", str(CELL / "values" / "k3d.yaml"), *every_image(),
        "--set", f"deployManager.researchWorkspace={named}", *(["--set", f"api.workspace={hosted}"] if hosted else []),
    )
    assert done.returncode != 0 and "researchWorkspace" in done.stderr, done.stderr
    paired = helm(
        CHART, "-f", str(CELL / "values" / "k3d.yaml"), *every_image(),
        "--set", "deployManager.researchWorkspace=01KDEM0000000000000000000Wr",
        "--set", "api.workspace=01KDEM0000000000000000000W",
        "--set", "modelGateway.workspace=01KDEM0000000000000000000Wr",
    )
    assert paired.returncode == 0, paired.stderr


def test_no_hosted_values_name_the_second_manager():
    """DEC-2070 item 4: no values file names a research workspace, and without one nothing of the
    second manager renders."""
    for values in sorted((CELL / "values").glob("*.yaml")) + sorted((CELL.parent / "gitops").rglob("*.yaml")):
        assert "researchWorkspace" not in values.read_text(), values
    assert not [d for d in render_all() if RESEARCH_MANAGER in json.dumps(d)]


def test_deploy_manager_contained():
    docs = render_all()
    assert deploy_manager_contained(docs) == []
    assert sorted(w["metadata"]["name"] for w in managed_pods(docs)) == ["executor", "runtime", EXCHANGE]
    for pod in managed_pods(docs):
        spec = pod_spec(pod)
        container = first_container(pod)
        if pod["metadata"]["name"] == EXCHANGE:
            assert container["command"] == ["/mandate-token-exchange"], container.get("command")
            assert container["args"] == ["run", "--vault-address", VAULT, "--vault-dir", "/run/mandate/vault"], container.get("args")
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
    assert helm(CHART, *paper_without_manager()).returncode != 0, "a paper executor needs the manager (DEC-2365)"


VAULT_CA = {
    "name": "vault-ca",
    "configMap": {"name": "vault-ca", "defaultMode": 292, "items": [{"key": "ca.crt", "path": "ca.crt"}]},
}
VAULT_CA_MOUNT = {"name": "vault-ca", "mountPath": "/run/mandate/vault", "readOnly": True}


def vault_ca_mounts(docs: list[dict], want: set[str]) -> list[str]:
    """The vault's CA certificate alone (the vault chart's ConfigMap `vault-ca`, never its key), at
    /run/mandate/vault read-only, in exactly the processes of `want`, managed or standing
    (DEC-2332 item 5)."""
    problems = []
    every = [*managed_pods(docs), *(w for w in workloads(docs) if w["kind"] == "Deployment")]
    for w in every:
        name = w["metadata"]["name"]
        spec = pod_spec(w)
        volumes = [
            v for v in spec.get("volumes", []) if v["name"] == "vault-ca" or v.get("configMap", {}).get("name") == "vault-ca"
        ]
        mounts = [m for c in all_containers({"spec": spec}) for m in c.get("volumeMounts", []) if m["name"] == "vault-ca"]
        expected = ([VAULT_CA], [VAULT_CA_MOUNT]) if name in want else ([], [])
        if (volumes, mounts) != expected:
            problems.append(f"{name} mounts {volumes} at {mounts}")
    return problems


def test_the_vault_ca_alone_is_mounted_read_only_where_the_vault_is_reached():
    docs = render_all()
    assert vault_ca_mounts(docs, {EXCHANGE}) == []
    exchange = next(w for w in managed_pods(docs) if w["metadata"]["name"] == EXCHANGE)
    assert first_container(exchange)["args"] == [
        "run", "--vault-address", VAULT, "--vault-dir", "/run/mandate/vault"
    ], "the exchange's launch, which its binary refuses any other form of"
    paper = render_all(*ALPACA_PAPER, "--set", f"executor.vaultAddress={VAULT}")
    assert vault_ca_mounts(paper, {EXCHANGE, "executor"}) == []
    assert deploy_manager_admission(paper) == [], "the executor's CA is admitted where it is mounted"
    assert vault_ca_mounts(render_all("--set", f"images.{MANAGER}="), set()) == [], "a fixture Deployment trusts no vault"
    keyed = mutate(
        docs, EXCHANGE, "Pod", lambda w: pod_spec(w)["volumes"][-1]["configMap"]["items"].append({"key": "tls.key", "path": "k"})
    )
    assert vault_ca_mounts(keyed, {EXCHANGE}), "the vault's key beside its CA is caught"
    writable = mutate(
        docs, EXCHANGE, "Pod", lambda w: next(m for m in first_container(w)["volumeMounts"] if m["name"] == "vault-ca").update(readOnly=False)
    )
    assert vault_ca_mounts(writable, {EXCHANGE}), "a writable CA mount is caught"
    assert vault_ca_mounts(render_all(*ALPACA_PAPER), {EXCHANGE}), "the paper executor without its CA is caught"


def vault_token_mounts(docs: list[dict]) -> dict[str, str]:
    """Each workload or manager template that mounts a ServiceAccount token projected for the
    vault (audience `mandate-vault`), by name, with its kind, Job, Deployment or the manager's Pod."""
    found = {}
    for w in workloads(docs):
        for v in pod_spec(w).get("volumes", []):
            sources = v.get("projected", {}).get("sources", [])
            if any(src.get("serviceAccountToken", {}).get("audience") == "mandate-vault" for src in sources):
                found[w["metadata"]["name"]] = w["kind"]
    return found


def test_only_pods_the_manager_starts_as_their_connections_account_mount_a_vault_token():
    """DEC-2336: a vault token is mounted only by a pod the deployment manager starts, which runs as
    its connection's own account; the executor's only where its broker reaches the vault. The
    chart's Deployment executor, the fixture broker's alone (DEC-2365), runs as `default`, which no
    vault role names, and mounts none."""
    assert vault_token_mounts(render_all()) == {EXCHANGE: "Pod"}
    paper = render_all(*ALPACA_PAPER, "--set", f"executor.vaultAddress={VAULT}")
    assert vault_token_mounts(paper) == {EXCHANGE: "Pod", "executor": "Pod"}
    unmanaged = render_all("--set", f"images.{MANAGER}=")
    assert vault_token_mounts(unmanaged) == {}, "the Deployment executor mounts no vault token"
    assert pod_spec(next(w for w in workloads(unmanaged) if w["metadata"]["name"] == "executor")).get(
        "serviceAccountName", "default"
    ) == "default"
    seeded = mutate(
        unmanaged,
        "executor",
        "Deployment",
        lambda w: pod_spec(w).setdefault("volumes", []).append(
            {"name": "v", "projected": {"sources": [{"serviceAccountToken": {"audience": "mandate-vault"}}]}}
        ),
    )
    assert vault_token_mounts(seeded) == {"executor": "Deployment"}, "a Deployment's vault token is caught"


def paper_without_manager() -> list[str]:
    """Staging's paper broker with every image but the deployment manager's."""
    return ["-f", str(CELL / "values" / "k3d.yaml"), *every_image(), *ALPACA_PAPER, "--set", f"images.{MANAGER}="]


def executor_kinds(chart: Path, *extra: str) -> dict[str, str] | None:
    """Where the executor renders with `extra`, by kind (`Deployment`, or `Pod` for the manager's
    template) and its account, or `None` when the chart refuses to render."""
    done = helm(chart, "-f", str(CELL / "values" / "k3d.yaml"), *every_image(), *extra)
    if done.returncode != 0:
        return None
    docs = documents(done.stdout)
    found = {
        w["kind"]: pod_spec(w).get("serviceAccountName", "default")
        for w in workloads(docs)
        if w["kind"] == "Deployment" and w["metadata"]["name"] == "executor"
    }
    found.update({"Pod": "manager's"} if any(p["metadata"]["name"] == "executor" for p in managed_pods(docs)) else {})
    return found


def a_paper_executor_runs_only_as_the_managers_pod(chart: Path) -> list[str]:
    """DEC-2365: an executor whose broker reaches the vault (`alpaca-paper`) renders only as the
    deployment manager's pod template, which the manager starts as its connection's own account
    (DEC-2336); without the manager the chart refuses to render, so no paper executor ever runs as
    `default` or under another connection's account. The fixture broker's executor stays a
    Deployment on `default`, with no connection and no credential."""
    problems = []
    paper = executor_kinds(chart, *ALPACA_PAPER)
    if paper != {"Pod": "manager's"}:
        problems.append(f"the paper executor with the manager renders as {paper}")
    unmanaged = executor_kinds(chart, *ALPACA_PAPER, "--set", f"images.{MANAGER}=")
    if unmanaged is not None:
        problems.append(f"the paper executor without the manager renders as {unmanaged}")
    else:
        refused = helm(chart, *paper_without_manager()).stderr
        if "DEC-2365" not in refused:
            problems.append(f"the refusal does not name its decision: {refused.strip()[-200:]}")
    fixture = executor_kinds(chart, "--set", f"images.{MANAGER}=")
    if fixture != {"Deployment": "default"}:
        problems.append(f"the fixture executor without the manager renders as {fixture}")
    return problems


def test_a_paper_executor_runs_only_as_the_managers_pod():
    assert a_paper_executor_runs_only_as_the_managers_pod(CHART) == []


def test_each_seeded_paper_executor_bug_is_caught(tmp_path):
    """Each bug seeded into a copy of the chart is caught, so the check above is not vacuous."""
    helpers = Path("templates") / "_helpers.tpl"
    services = Path("templates") / "services.yaml"
    bugs = {
        "no refusal": (services, "{{- fail", "{{- /* fail */ -}}{{- $_ := list"),
        "the paper broker on default": (
            services,
            '{{- if eq .root.Values.executor.broker "alpaca-paper" }}',
            '{{- if eq .root.Values.executor.broker "none" }}',
        ),
        "the fixture refused too": (
            services,
            '{{- if eq .root.Values.executor.broker "alpaca-paper" }}',
            "{{- if true }}",
        ),
        "another account on the Deployment": (
            helpers,
            "  automountServiceAccountToken: false\n",
            "  serviceAccountName: \"0123456789abcdef\"\n  automountServiceAccountToken: false\n",
        ),
    }
    for why, (path, old, new) in bugs.items():
        copy = tmp_path / why.replace(" ", "-")
        shutil.copytree(CHART, copy)
        text = (copy / path).read_text()
        assert old in text, f"{why}: {old!r} is not in {path}"
        (copy / path).write_text(text.replace(old, new, 1))
        assert a_paper_executor_runs_only_as_the_managers_pod(copy), f"{why} is not caught"


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
        and d["metadata"]["name"] not in ("migrations", "dispatcher-config")
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
# The names the manager's adapter adds to each of a pod's containers, its init containers aside
# (crates/mandate-deploy-manager kubernetes.rs); a token exchange's also names its account and its
# request's expiry (DEC-2363).
MANAGER_ENV = ["MANDATE_WORKSPACE", "MANDATE_PROCESS", "MANDATE_SUBJECT", "MANDATE_GENERATION"]
# The connection a test's token exchange is for, and the account DEC-2060 names for it: `x` and the
# id's bytes in lowercase hex.
CONNECTION = "conn_01J8Z2AB"
CONNECTION_ACCOUNT = "x" + CONNECTION.encode().hex()
# The account an executor for the same connection runs as: the bare hex (DEC-2060, DEC-2336), and
# the names its environment adds beside a token exchange's.
EXECUTOR_ACCOUNT = CONNECTION.encode().hex()
OWN_ENV = {
    EXCHANGE: ["MANDATE_SUBJECT_ACCOUNT", "MANDATE_REQUEST_EXPIRES_AT"],
    "executor": ["MANDATE_SUBJECT_ACCOUNT", "MANDATE_WORKSPACE_NAMESPACE"],
}
# The request expiry a test's token exchange is given (DEC-2363), in the journal's form of a time.
REQUEST_EXPIRES_AT = "2026-10-17T15:10:10.000000000Z"
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
    "a pod the deployment manager makes names no service account but the default, or a token exchange or an executor its own": [
        "serviceAccountName",
        'object.spec.serviceAccountName == "default"',
        '"token-exchange"',
        "matches('^x([0-9a-f]{2}){1,64}$')",
        '"executor"',
        "matches('^([0-9a-f]{2}){1,64}$')",
    ],
    "a token exchange's or an executor's pod names its own account in its environment once, beside its subject, which the process checks against its connection": [
        '["token-exchange", "executor"]',
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
    "a pod the deployment manager makes mounts only memory, its own process's Secrets, vault CA and projected token, or its claims read-only": [
        "has(v.emptyDir)",
        "v.secret.secretName in variables.secrets[variables.process]",
        "in variables.configMaps[variables.process]",
        "v.configMap.optional == false",
        "v.persistentVolumeClaim.claimName in variables.claims[variables.process]",
        "v.persistentVolumeClaim.readOnly == true",
        "size(v.projected.sources) == 2",
        "size(v.projected.sources[1].downwardAPI.items) == 1",
        "in variables.projected[variables.process]",
    ],
    "every container of a pod the deployment manager makes has exactly its template's environment, and the manager's own names once each": [
        "variables.env[variables.process][c.name].all(f,",
        "size(variables.added[variables.process][c.name])",
        "size(c.env.filter(e, e.name == n)) == 1",
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
    it before any policy runs; a token exchange's and an executor's account their connection's
    (DEC-2060, DEC-2336), which their environment names beside their subject (DEC-1177), and an
    executor's namespace too."""
    pod = copy.deepcopy(pod)
    pod["metadata"]["labels"].update(ADDED_LABELS)
    name = pod["metadata"]["labels"]["mandate.dev/process"]
    exchange = name == EXCHANGE
    own = {EXCHANGE: CONNECTION_ACCOUNT, "executor": EXECUTOR_ACCOUNT}.get(name)
    pod["spec"]["serviceAccountName"] = own or "default"
    process_code = pod["metadata"]["labels"]["mandate.dev/process"].replace("-", "_")
    for c in pod["spec"]["containers"]:
        c.setdefault("env", []).extend(
            {"name": n, "value": v}
            for n, v in (
                ("MANDATE_WORKSPACE", "01J8Z2WS000000000000000000"),
                ("MANDATE_PROCESS", process_code),
                ("MANDATE_SUBJECT", CONNECTION if exchange else "subject_1"),
                ("MANDATE_GENERATION", "1"),
            )
        )
        if own:
            c["env"].append({"name": "MANDATE_SUBJECT_ACCOUNT", "value": own})
        if name == "executor":
            c["env"].append({"name": "MANDATE_WORKSPACE_NAMESPACE", "value": "ws-ws1"})
        if exchange:
            c["env"].append({"name": "MANDATE_REQUEST_EXPIRES_AT", "value": REQUEST_EXPIRES_AT})
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
    vault_ca = lambda b: next(v for v in spec(b)["volumes"] if v["name"] == "vault-ca")["configMap"]  # noqa: E731
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
        changed(
            "no request expiry in its environment",
            lambda b: first(b)["env"].remove(env(b, "MANDATE_REQUEST_EXPIRES_AT")),
        ),
        changed(
            "a second request expiry in its environment",
            lambda b: first(b)["env"].append({"name": "MANDATE_REQUEST_EXPIRES_AT", "value": REQUEST_EXPIRES_AT}),
        ),
        changed("no namespace file", lambda b: projected(b)["sources"].pop(1)),
        changed("an optional vault CA", lambda b: vault_ca(b).update(optional=True)),
        changed("the vault's key beside its CA", lambda b: vault_ca(b)["items"].append({"key": "tls.key", "path": "tls.key"})),
        changed("the vault CA's own mode", lambda b: vault_ca(b)["items"][0].update(mode=511)),
        changed("a wider vault CA mode", lambda b: vault_ca(b).update(defaultMode=511)),
        changed("another CA", lambda b: vault_ca(b).update(name="journal-ca")),
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
    tokened = any("projected" in v for v in spec(pod).get("volumes", []))
    executor_only = lambda: [  # noqa: E731
        changed("the default account", lambda b: spec(b).update(serviceAccountName="default")),
        changed("no account", lambda b: spec(b).pop("serviceAccountName")),
        changed("an exchange's account", lambda b: spec(b).update(serviceAccountName=CONNECTION_ACCOUNT)),
        changed(
            "the token exchange's labels",
            lambda b: (
                [labels(b).pop(k) for k in sorted(egress_labels(labels(b)))],
                labels(b).update(as_exchange),
                spec(b).update(serviceAccountName=CONNECTION_ACCOUNT),
            ),
        ),
        changed("an odd count of hex digits", lambda b: spec(b).update(serviceAccountName=EXECUTOR_ACCOUNT[:-1])),
        changed("an upper-case account", lambda b: spec(b).update(serviceAccountName=EXECUTOR_ACCOUNT.upper())),
        changed("an account longer than any id", lambda b: spec(b).update(serviceAccountName="ab" * 65)),
        changed("an account its environment does not name", lambda b: env(b, "MANDATE_SUBJECT_ACCOUNT").update(value="abab")),
        changed(
            "no account in its environment",
            lambda b: first(b)["env"].remove(env(b, "MANDATE_SUBJECT_ACCOUNT")),
        ),
        changed(
            "a second account in its environment",
            lambda b: first(b)["env"].append({"name": "MANDATE_SUBJECT_ACCOUNT", "value": EXECUTOR_ACCOUNT}),
        ),
        changed(
            "no namespace in its environment",
            lambda b: first(b)["env"].remove(env(b, "MANDATE_WORKSPACE_NAMESPACE")),
        ),
        changed(
            "a second namespace in its environment",
            lambda b: first(b)["env"].append({"name": "MANDATE_WORKSPACE_NAMESPACE", "value": "ws-ws2"}),
        ),
        *(
            [
                changed("no namespace file", lambda b: projected(b)["sources"].pop(1)),
                changed("a longer-lived token", lambda b: projected(b)["sources"][0]["serviceAccountToken"].update(expirationSeconds=86400)),
                changed("a token for another audience", lambda b: projected(b)["sources"][0]["serviceAccountToken"].update(audience="kubernetes")),
            ]
            if tokened
            else [changed("the vault token", lambda b: spec(b)["volumes"].append({"name": "v", "projected": vault_token}))]
        ),
    ]
    others_only = lambda: [  # noqa: E731
        changed("an exchange's account", lambda b: spec(b).update(serviceAccountName="x0123456789abcdef")),
        changed("an executor's account", lambda b: spec(b).update(serviceAccountName=EXECUTOR_ACCOUNT)),
        changed(
            "an executor's account named in its environment",
            lambda b: (
                spec(b).update(serviceAccountName=EXECUTOR_ACCOUNT),
                first(b)["env"].append({"name": "MANDATE_SUBJECT_ACCOUNT", "value": EXECUTOR_ACCOUNT}),
            ),
        ),
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
        *(
            exchange_only()
            if process_name == EXCHANGE
            else executor_only()
            if process_name == "executor"
            else others_only()
        ),
        *(
            []
            if process_name == EXCHANGE
            else [
                changed(
                    "a request expiry on a process that is not an exchange",
                    lambda b: first(b)["env"].append(
                        {"name": "MANDATE_REQUEST_EXPIRES_AT", "value": REQUEST_EXPIRES_AT}
                    ),
                )
            ]
        ),
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
        changed(
            "the cluster's CA ConfigMap",
            lambda b: spec(b)["volumes"].append(
                {"name": "k", "configMap": {"name": "kube-root-ca.crt", "defaultMode": 292, "items": [{"key": "ca.crt", "path": "ca.crt"}]}}
            ),
        ),
        changed(
            "another process's ConfigMap",
            lambda b: spec(b)["volumes"].append(
                {"name": "t", "configMap": {"name": "deploy-manager-templates", "defaultMode": 292, "items": [{"key": "runtime.json", "path": "r"}]}}
            ),
        ),
        changed(
            "another token host",
            lambda b: first(b)["env"].append({"name": "MANDATE_TOKEN_URL", "value": "https://evil.example/t"}),
        ),
        changed(
            "another tier",
            lambda b: next(e for e in first(b)["env"] if e["name"] == "MANDATE_DEPLOYMENT_TIER").update(value="dev"),
        ),
        changed(
            "a template value set again",
            lambda b: first(b)["env"].append(
                {**next(e for e in first(b)["env"] if e["name"] == "MANDATE_DEPLOYMENT_TIER"), "value": "dev"}
            ),
        ),
        changed(
            "a manager name set twice",
            lambda b: first(b)["env"].append({"name": "MANDATE_PROCESS", "value": "executor"}),
        ),
        changed(
            "a manager name missing",
            lambda b: first(b)["env"].remove(next(e for e in first(b)["env"] if e["name"] == "MANDATE_GENERATION")),
        ),
        changed(
            "a template name missing",
            lambda b: first(b)["env"].remove(next(e for e in first(b)["env"] if e["name"] == "HOME")),
        ),
        changed(
            "an init container's environment",
            lambda b: spec(b)["initContainers"][0].setdefault("env", []).append({"name": "MANDATE_SUBJECT", "value": "x"}),
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
        "claims": {n: ["artifacts"] if n in ("runtime", "executor") and claimed else [] for n in names},
        "projected": {
            n: ["mandate-vault|600|token|292|namespace=metadata.namespace"]
            if n == EXCHANGE or (n == "executor" and "alpaca-paper" in json.dumps(launches(templates[n])))
            else []
            for n in names
        },
        "egress": {n: [f"mandate.dev/egress-{t}" for t in process(n)["egress"]] for n in names},
        "configMaps": {
            n: [
                f"{v['configMap']['name']}|{v['configMap']['defaultMode']}|"
                f"{v['configMap']['items'][0]['key']}={v['configMap']['items'][0]['path']}"
                for v in t["spec"].get("volumes", [])
                if "configMap" in v
            ]
            for n, t in templates.items()
        },
        "env": {
            n: {
                c["name"]: [f"{e['name']}={e.get('value', '')}" for e in c.get("env", [])]
                for c in all_containers(t)
            }
            for n, t in templates.items()
        },
        "added": {
            n: {
                **{c["name"]: [] for c in t["spec"].get("initContainers", [])},
                **{c["name"]: MANAGER_ENV + OWN_ENV.get(n, []) for c in t["spec"]["containers"]},
            }
            for n, t in templates.items()
        },
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
    "an account the deployment manager makes or deletes is named for a connection's token exchange or executor": [
        "variables.account.metadata.name.matches('^x([0-9a-f]{2}){1,64}$')",
        "variables.account.metadata.name.matches('^([0-9a-f]{2}){1,64}$')",
    ],
    "an account the deployment manager makes or deletes mounts no token and holds no Secret": [
        "variables.account.automountServiceAccountToken == false",
        "!has(variables.account.secrets)",
        "!has(variables.account.imagePullSecrets)",
    ],
    "an account the deployment manager makes or deletes is a token exchange's or an executor's, as its name says, in this workspace": [
        '["app.kubernetes.io/managed-by"] == "mandate-deploy-manager"',
        '["mandate.dev/workspace"] == "ws1"',
        '["mandate.dev/process"] == (variables.account.metadata.name.startsWith("x") ? "token-exchange" : "executor")',
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
        changed(
            "an executor's name with a runtime's label",
            lambda b: (meta(b).update(name=EXECUTOR_ACCOUNT), labels(b).update({"mandate.dev/process": "runtime"})),
        ),
        changed(
            "an odd count of hex digits for an executor",
            lambda b: (meta(b).update(name=EXECUTOR_ACCOUNT[:-1]), labels(b).update({"mandate.dev/process": "executor"})),
        ),
        changed(
            "an upper-case executor's name",
            lambda b: (meta(b).update(name=EXECUTOR_ACCOUNT.upper()), labels(b).update({"mandate.dev/process": "executor"})),
        ),
        changed(
            "an executor's name longer than any id",
            lambda b: (meta(b).update(name="ab" * 65), labels(b).update({"mandate.dev/process": "executor"})),
        ),
        changed(
            "an executor's account in another workspace",
            lambda b: (
                meta(b).update(name=EXECUTOR_ACCOUNT),
                labels(b).update({"mandate.dev/process": "executor", "mandate.dev/workspace": "ws2"}),
            ),
        ),
        changed(
            "an executor's account with a mounted token",
            lambda b: (
                meta(b).update(name=EXECUTOR_ACCOUNT),
                labels(b).update({"mandate.dev/process": "executor"}),
                b.update(automountServiceAccountToken=True),
            ),
        ),
    ]


def deploy_manager_accounts(docs: list[dict]) -> list[str]:
    """The manager's `create` and `delete serviceaccounts` are bounded as its pods are (DEC-1177):
    one policy and its binding exactly when the manager renders, failing closed, matching core
    ServiceAccounts' create, update and delete in the namespace and only the manager's own
    requests, denying, with every bound in ACCOUNT_BOUNDS. A token exchange's and an executor's
    account as the manager makes them pass on create, update and delete (DEC-2336); each attack fails on each, so the manager
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
        for kind, made in (("exchange", account()), ("executor", account(EXECUTOR_ACCOUNT, "executor"))):
            if account_refusals(policy, made, operation):
                problems.append(f"the account policy refuses the {kind}'s {operation}: {account_refusals(policy, made, operation)}")
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
        drop_rule("an account the deployment manager makes or deletes is named for a connection's token exchange or executor"),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        rewrite_rule(
            "an account the deployment manager makes or deletes is named for a connection's token exchange or executor",
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
        drop_rule("an account the deployment manager makes or deletes is a token exchange's or an executor's, as its name says, in this workspace"),
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
        loosen("an account the deployment manager makes or deletes is named for a connection's token exchange or executor"),
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
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        rewrite_rule(
            "an account the deployment manager makes or deletes is named for a connection's token exchange or executor",
            "matches('^([0-9a-f]{2}){1,64}$')",
            "matches('^[0-9a-f]+$')",
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d, ACCOUNTS_POLICY),
        POLICY,
        rewrite_rule(
            "an account the deployment manager makes or deletes is a token exchange's or an executor's, as its name says, in this workspace",
            '== (variables.account.metadata.name.startsWith("x") ? "token-exchange" : "executor")',
            'in ["token-exchange", "executor"]',
        ),
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
        "Pod",
        lambda w: first_container(w).update(
            env=[e for e in first_container(w)["env"] if e["name"] != "MANDATE_ALPACA_REDIRECT_URI"]
        ),
    )
    assert alpaca_oauth_env(dropped, ALPACA_SET_VALUES), "the exchange without the redirect URI is caught"
    differs = mutate(
        docs,
        "token-exchange",
        "Pod",
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
    manager_workspace = ["--set", "api.workspace=01KDEM0000000000000000000W"]
    dispatcher = ["--set", f"images.dispatcher={PLACEHOLDER.format('dispatcher')}"]
    without_api = [
        arg
        for pair in zip(images[0::2], images[1::2])
        if list(pair) not in (api, dispatcher)
        for arg in pair
    ] + manager_workspace
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
    lambda d: mutate(d, policy_of(d), POLICY, widen("configMaps", "runtime", "kube-root-ca.crt|292|ca.crt=ca.crt")),
    lambda d: mutate(d, policy_of(d), POLICY, widen("configMaps", EXCHANGE, "vault-ca|292|tls.key=tls.key")),
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
            "a pod the deployment manager makes names no service account but the default, or a token exchange or an executor its own",
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
            "a pod the deployment manager makes mounts only memory, its own process's Secrets, vault CA and projected token, or its claims read-only"
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        loosen(
            "a token exchange's or an executor's pod names its own account in its environment once, beside its subject, which the process checks against its connection"
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
        loosen("a pod the deployment manager makes names no service account but the default, or a token exchange or an executor its own"),
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
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        loosen(
            "every container of a pod the deployment manager makes has exactly its template's environment, and the manager's own names once each"
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        replace_variable(
            "added", lambda added: {p: {c: [*names, "MANDATE_TOKEN_URL"] for c, names in cs.items()} for p, cs in added.items()}
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        rewrite_rule(
            "a pod the deployment manager makes names no service account but the default, or a token exchange or an executor its own",
            "matches('^([0-9a-f]{2}){1,64}$')",
            "matches('^[0-9a-f]+$')",
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        rewrite_rule(
            "a token exchange's or an executor's pod names its own account in its environment once, beside its subject, which the process checks against its connection",
            '["token-exchange", "executor"]',
            '["token-exchange"]',
        ),
    ),
    lambda d: mutate(
        d,
        policy_of(d),
        POLICY,
        replace_variable(
            "added", lambda added: {p: {c: [n for n in names if n != "MANDATE_WORKSPACE_NAMESPACE"] for c, names in cs.items()} for p, cs in added.items()}
        ),
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
