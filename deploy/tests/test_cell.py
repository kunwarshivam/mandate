"""Invariants of the workspace chart and the Compose file (E21-36), on what they render.

Each property of the story's acceptance criteria is one function returning the problems it finds,
asserted empty on the real rendering and shown non-empty on a seeded bug in
`test_each_seeded_bug_is_caught`. The chart emits one JSON document per manifest, so no YAML parser
is needed. Without helm these tests skip, unless MANDATE_CELL_HELM_REQUIRED is set, as CI's cell job
sets it (`cargo xtask ci cell`).
"""

import copy
import json
import os
import re
import shutil
import subprocess
from pathlib import Path

import pytest

CELL = Path(__file__).resolve().parents[1] / "cell"
CHART = CELL / "charts" / "workspace"
JOURNAL_CHART = CELL / "charts" / "journal-db"
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
    "control": {("postgres", "TCP", 5432), ("vault", "TCP", 8200), ("identity-provider", "TCP", 443)},
    "journal-verify": {("postgres", "TCP", 5432)},
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
}
HOOK = "pre-install,pre-upgrade"

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
        *extra,
    )


def process(name: str) -> dict:
    return next(p for p in PROCESSES if p["name"] == name)


def workloads(docs: list[dict]) -> list[dict]:
    return [d for d in docs if d["kind"] in ("Deployment", "Job")]


def pod_spec(workload: dict) -> dict:
    return workload["spec"]["template"]["spec"]


def containers(workload: dict) -> list[dict]:
    spec = pod_spec(workload)
    return [*spec.get("initContainers", []), *spec["containers"]]


def owner(workload: dict) -> dict:
    return process(workload["metadata"]["labels"]["mandate.dev/process"])


def policies(docs: list[dict]) -> dict[str, dict]:
    return {d["metadata"]["name"]: d for d in docs if d["kind"] == "NetworkPolicy"}


def one_workload_per_process(docs: list[dict]) -> list[str]:
    """Every process type renders exactly one Deployment (a service) or Job, and nothing else."""
    kinds = {"service": "Deployment", "migration": "Job", "run": "Job", "verify": "Job"}
    problems = []
    for p in PROCESSES:
        found = [
            w for w in workloads(docs) if w["metadata"]["labels"].get("mandate.dev/process") == p["name"]
        ]
        if [w["kind"] for w in found] != [kinds[p["kind"]]]:
            problems.append(f"{p['name']}: {[w['kind'] for w in found]}")
    return problems


def colocated(docs: list[dict]) -> list[str]:
    """A workspace's executors, runtimes and data service share a node by required pod affinity (§3.2)."""
    problems = []
    for w in workloads(docs):
        affinity = pod_spec(w).get("affinity", {}).get("podAffinity", {})
        terms = affinity.get("requiredDuringSchedulingIgnoredDuringExecution", [])
        want = owner(w)["colocate"]
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
    cidrs = cidrs or {}
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
    """Every pod non-root with the runtime's seccomp profile and no service-account token; every
    container read-only, unprivileged, with every capability dropped."""
    problems = []
    for w in workloads(docs):
        spec = pod_spec(w)
        sc = spec.get("securityContext", {})
        if not (
            sc.get("runAsNonRoot") is True
            and sc.get("runAsUser", 0) != 0
            and sc.get("seccompProfile") == {"type": "RuntimeDefault"}
            and spec.get("automountServiceAccountToken") is False
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
    pod's own egress is shown filtered, and fails closed after a minute (AGENTS.md rule 3)."""
    problems = []
    for w in workloads(docs):
        first = (pod_spec(w).get("initContainers") or [{}])[0]
        script = " ".join(first.get("command", []))
        if (
            first.get("name") != "egress-gate"
            or "KUBERNETES_SERVICE_HOST" not in script
            or "exit 1" not in script
        ):
            problems.append(f"{w['metadata']['name']} starts before its egress is shown filtered")
    return problems


SECRET_KEY = re.compile(
    r"(password|passwd|secret|apikey|api[_-]?key|credential|token$|token_|_token)", re.IGNORECASE
)


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
                if SECRET_KEY.search(e["name"]) and e["name"] != "PGPASSFILE":
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


ALLOWED_APIS = {"v1", "apps/v1", "batch/v1", "networking.k8s.io/v1"}


def portable(docs: list[dict]) -> list[str]:
    """No k3s-only, provider-only or cloud-only API (DEC-967 item 6): core Kubernetes APIs only, no
    storage class, node name or ingress class in the workspace chart."""
    problems = [f"{d['kind']} uses {d['apiVersion']}" for d in docs if d["apiVersion"] not in ALLOWED_APIS]
    text = json.dumps(docs)
    for word in ("storageClassName", "nodeName", "nodeSelector", "ingressClassName", "cattle.io", "hetzner", "digitalocean"):
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
    built = {p["name"] for p in PROCESSES if p["built"]}
    problems = (
        []
        if set(services) - {"postgres"} == built
        else [f"services {sorted(services)}, built {sorted(built)}"]
    )
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
            if (SECRET_KEY.search(key) and key != "PGPASSFILE") or re.search(r"://[^/@]*:[^/@]*@", value):
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


def test_a_process_renders_only_with_its_image():
    docs = render("-f", str(CELL / "values" / "k3d.yaml"))
    assert {w["metadata"]["name"] for w in workloads(docs)} == {"migrate"}
    assert set(policies(docs)) == {"default-deny", "egress-migrate"}


def test_each_service_starts_and_is_probed_as_its_process_list_says():
    """A service with `args` starts with them and is asked `readyz` and `livez` with its `probe`, every
    placeholder filled from the values (DEC-1013, DEC-994 item 5)."""
    docs = render_all()
    for p in PROCESSES:
        if p["kind"] != "service" or not (p.get("args") or p.get("probe")):
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
    extra = [arg for t, c in cidrs.items() for arg in ("--set-json", f'egress.{t}=["{c}"]')]
    docs = render_all(*extra)
    assert egress_exact(docs, cidrs) == []
    token = [n for n, p in policies(docs).items() if "192.0.2.2/32" in json.dumps(p)]
    assert token == ["egress-token-exchange"], "DEC-821's token host is the token-exchange process's alone"


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


def without_ingress(docs: list[dict]) -> list[dict]:
    return [d for d in docs if d["metadata"]["name"] not in ("cloudflared", "egress-cloudflared")]


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


def test_environment_is_paper_or_backtest():
    assert render("--set", "environment=backtest")
    for refused in ("live", "production", ""):
        assert helm(CHART, "--set", f"environment={refused}").returncode != 0, refused


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
    assert helm(JOURNAL_CHART, "--set", "superuserPassword=x").returncode != 0


def test_a_stopping_journal_waits_briefly_for_its_pooled_clients():
    """The runtime's pool never disconnects by itself, so CloudNativePG's default smart shutdown
    (180 s) would hold every primary restart that long before the fast one (DEC-1013 item 6)."""
    cluster = next(d for d in render(chart=JOURNAL_CHART) if d["kind"] == "Cluster")
    assert cluster["spec"]["smartShutdownTimeout"] == 10
    assert helm(JOURNAL_CHART, "--set", "smartShutdownTimeout=600").returncode != 0


def test_roles_match():
    assert roles_match(render(chart=JOURNAL_CHART)) == []


def test_compose_matches():
    assert compose_matches(COMPOSE) == []


def mutate(docs: list[dict], name: str, kind: str, change) -> list[dict]:
    docs = copy.deepcopy(docs)
    target = next(d for d in docs if d["metadata"]["name"] == name and d["kind"] == kind)
    change(target)
    return docs


def first_container(w: dict) -> dict:
    return pod_spec(w)["containers"][0]


SEEDED = [
    (one_workload_per_process, lambda d: [x for x in d if x["metadata"]["name"] != "scheduler"]),
    (colocated, lambda d: mutate(d, "executor", "Deployment", lambda w: pod_spec(w).pop("affinity"))),
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
        lambda d: mutate(
            d, "executor", "Deployment", lambda w: pod_spec(w)["securityContext"].update(runAsUser=0)
        ),
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
            "Deployment",
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
            "Deployment",
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
        lambda d: mutate(
            d, "executor", "Deployment", lambda w: first_container(w).update(args=["db", "migrate"])
        ),
    ),
    (
        portable,
        lambda d: [*d, {"kind": "HelmChart", "apiVersion": "helm.cattle.io/v1", "metadata": {"name": "x"}}],
    ),
    (
        portable,
        lambda d: mutate(
            d, "executor", "Deployment", lambda w: pod_spec(w).update(nodeSelector={"node": "pool-1"})
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


def test_roles_and_compose_seeded_bugs_are_caught():
    journal = render(chart=JOURNAL_CHART)
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
    ):
        compose = copy.deepcopy(COMPOSE)
        change(compose)
        assert compose_matches(compose), change
