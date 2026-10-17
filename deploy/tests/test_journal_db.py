"""Invariants of the journal's Postgres chart and the charts beside it (E21-41), on what they render.

As in test_cell.py, each property is one function returning the problems it finds, asserted empty on
the real rendering and shown non-empty on a seeded bug. The expected settings are written out here
from the story and DEC-1050, not read from the chart, so a chart that drifts fails.
"""

import copy
import json
import os
import shutil
import subprocess
from pathlib import Path

import pytest

CELL = Path(__file__).resolve().parents[1] / "cell"
JOURNAL_CHART = CELL / "charts" / "journal-db"
STORE_CHART = CELL / "charts" / "archive-store"
TLS_CHART = CELL / "charts" / "backup-plugin-tls"
K3D_VALUES = CELL / "values" / "k3d-journal-db.yaml"
KUBE_API = ["--set-json", 'kubeApi={"cidrs":["10.43.0.1/32"],"ports":[443]}']
PLUGIN = "barman-cloud.cloudnative-pg.io"
# The story's synchronous standby: one standby out of the others, and still asked for when it is
# down, so a commit waits rather than lands on the primary alone (OPS-2).
SYNCHRONOUS = {"method": "any", "number": 1, "dataDurability": "required"}
PEER_PORTS = {5432, 8000}

if shutil.which("helm") is None:
    if os.environ.get("MANDATE_CELL_HELM_REQUIRED"):
        raise RuntimeError("MANDATE_CELL_HELM_REQUIRED is set but helm is not installed")
    pytest.skip("helm is not installed (deploy/cell/README.md)", allow_module_level=True)


def helm(chart: Path, namespace: str, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["helm", "template", "cell", str(chart), "--namespace", namespace, *args],
        capture_output=True,
        text=True,
        check=False,
    )


def documents(text: str) -> list[dict]:
    lines = (line.strip() for line in text.splitlines())
    return [json.loads(line) for line in lines if line and line != "---" and not line.startswith("#")]


def render(chart: Path, namespace: str, *args: str) -> list[dict]:
    done = helm(chart, namespace, *args)
    assert done.returncode == 0, done.stderr
    return documents(done.stdout)


def journal(*args: str) -> list[dict]:
    return render(JOURNAL_CHART, "ws-ws1", "-f", str(K3D_VALUES), *KUBE_API, *args)


def journal_refused(*args: str) -> bool:
    return helm(JOURNAL_CHART, "ws-ws1", "-f", str(K3D_VALUES), *KUBE_API, *args).returncode != 0


def one(docs: list[dict], kind: str, name: str | None = None) -> dict | None:
    found = [d for d in docs if d["kind"] == kind and (name is None or d["metadata"]["name"] == name)]
    return found[0] if len(found) == 1 else None


def standby_synchronous(docs: list[dict]) -> list[str]:
    """Two instances or more, `synchronous_commit = on`, and one standby every commit waits for,
    still waited for with that standby down."""
    cluster = one(docs, "Cluster", "journal")
    if cluster is None:
        return ["no one Cluster `journal`"]
    spec = cluster["spec"]
    postgresql = spec.get("postgresql", {})
    problems = []
    if spec.get("instances", 0) < 2:
        problems.append(f"instances {spec.get('instances')}, want a primary and a standby")
    if postgresql.get("parameters", {}).get("synchronous_commit") != "on":
        problems.append("synchronous_commit is not on")
    if postgresql.get("synchronous") != SYNCHRONOUS:
        problems.append(f"synchronous {postgresql.get('synchronous')}, want {SYNCHRONOUS}")
    for legacy in ("minSyncReplicas", "maxSyncReplicas"):
        if legacy in spec:
            problems.append(f"{legacy} set beside `synchronous`")
    return problems


def archive_configured(docs: list[dict]) -> list[str]:
    """The WAL archiver is the Barman Cloud plugin, writing to the chart's ObjectStore with a key no
    database role holds, and a scheduled base backup goes through the same plugin."""
    cluster = one(docs, "Cluster", "journal")
    store = one(docs, "ObjectStore")
    backup = one(docs, "ScheduledBackup")
    if cluster is None or store is None or backup is None:
        return ["the Cluster, one ObjectStore and one ScheduledBackup are not all rendered"]
    problems = []
    archivers = [p for p in cluster["spec"].get("plugins", []) if p.get("isWALArchiver")]
    if len(archivers) != 1 or archivers[0]["name"] != PLUGIN:
        problems.append(f"WAL archivers {archivers}, want the Barman Cloud plugin alone")
    elif archivers[0].get("parameters", {}).get("barmanObjectName") != store["metadata"]["name"]:
        problems.append("the archiver names another ObjectStore")
    if "backup" in cluster["spec"]:
        problems.append("the Cluster also configures the operator's in-tree backup")
    config = store["spec"]["configuration"]
    credentials = config.get("s3Credentials", {})
    secrets = {credentials.get(k, {}).get("name") for k in ("accessKeyId", "secretAccessKey")}
    roles = {d["metadata"]["name"] for d in docs if d["kind"] == "Secret"}
    if len(secrets) != 1 or None in secrets:
        problems.append(f"the archive's key comes from {secrets}, want one Secret")
    elif secrets & roles:
        problems.append(f"the archive's key is a database credential: {secrets & roles}")
    if credentials.get("inheritFromIAMRole"):
        problems.append("the archive's key is inherited from the node")
    if not config.get("destinationPath", "").startswith("s3://"):
        problems.append("the archive's destination is not an s3:// path")
    spec = backup["spec"]
    if (
        spec.get("method") != "plugin"
        or spec.get("pluginConfiguration", {}).get("name") != PLUGIN
        or spec.get("cluster", {}).get("name") != "journal"
    ):
        problems.append(f"the scheduled base backup is {spec}")
    return problems


def peer_label(peer: dict) -> str:
    if "ipBlock" in peer:
        return "cidr"
    labels = peer.get("podSelector", {}).get("matchLabels", {})
    namespace = peer.get("namespaceSelector", {}).get("matchLabels", {}).get("kubernetes.io/metadata.name")
    if namespace == "kube-system" and labels == {"k8s-app": "kube-dns"}:
        return "dns"
    if namespace is None and labels == {"cnpg.io/cluster": "journal"}:
        return "peers"
    if namespace == "archive-store" and labels == {"app.kubernetes.io/name": "mandate-archive-store"}:
        return "archive"
    return f"other:{peer}"


def egress_narrow(docs: list[dict]) -> list[str]:
    """The journal's pods reach the cluster DNS, the Kubernetes API by /32 addresses, each other on
    5432 and 8000, and the archive, each on named ports, and nothing else."""
    policy = one(docs, "NetworkPolicy", "journal")
    if policy is None:
        return ["no network policy `journal`"]
    if "Egress" not in policy["spec"].get("policyTypes", []):
        return ["the journal's policy does not filter egress"]
    problems = []
    seen = set()
    for rule in policy["spec"].get("egress", []):
        peers, ports = rule.get("to", []), rule.get("ports", [])
        if not peers or not ports:
            problems.append(f"an egress rule open to every peer or port: {rule}")
            continue
        numbers = {p["port"] for p in ports}
        for peer in peers:
            label = peer_label(peer)
            seen.add(label)
            if label == "cidr" and not peer["ipBlock"]["cidr"].endswith("/32"):
                problems.append(f"a CIDR wider than one address: {peer['ipBlock']['cidr']}")
            if label == "dns" and numbers != {53}:
                problems.append(f"DNS on ports {numbers}")
            if label == "peers" and numbers != PEER_PORTS:
                problems.append(f"peers on ports {numbers}")
            if label == "archive" and numbers != {9000}:
                problems.append(f"the archive on ports {numbers}")
            if label.startswith("other:"):
                problems.append(f"an egress peer outside the journal's: {label}")
    for needed in ("dns", "cidr", "peers", "archive"):
        if needed not in seen:
            problems.append(f"no egress to {needed}, which the journal needs")
    return problems


def failure_domain_disclosed(docs: list[dict], shared: bool) -> list[str]:
    """Separate failure domains are a scheduling rule; a shared one is said in the Cluster itself."""
    cluster = one(docs, "Cluster", "journal")
    if cluster is None:
        return ["no Cluster"]
    affinity = cluster["spec"].get("affinity", {})
    said = cluster["metadata"].get("annotations", {}).get("mandate.dev/failure-domain", "")
    problems = []
    if (
        affinity.get("enablePodAntiAffinity") is not True
        or affinity.get("topologyKey") != "kubernetes.io/hostname"
    ):
        problems.append(f"no anti-affinity by node: {affinity}")
    want = "preferred" if shared else "required"
    if affinity.get("podAntiAffinityType") != want:
        problems.append(f"anti-affinity {affinity.get('podAntiAffinityType')}, want {want}")
    if shared and "no second failure domain" not in said:
        problems.append(f"a shared failure domain not disclosed: `{said}`")
    if not shared and not said.startswith("separate"):
        problems.append(f"separate failure domains said as `{said}`")
    return problems


def store_isolated(docs: list[dict]) -> list[str]:
    """The test store accepts only the journal's archive clients, reaches nothing, runs hardened, and
    hands the journal's namespace its key as the names the ObjectStore reads."""
    policy = one(docs, "NetworkPolicy", "archive-store")
    deployment = one(docs, "Deployment", "archive-store")
    client = one(docs, "Secret", "journal-archive-key")
    if policy is None or deployment is None or client is None:
        return ["the store's policy, Deployment or client key is not rendered once"]
    problems = []
    spec = policy["spec"]
    if set(spec.get("policyTypes", [])) != {"Ingress", "Egress"} or spec.get("egress") != []:
        problems.append("the store reaches out")
    want_from = [
        {
            "namespaceSelector": {"matchLabels": {"kubernetes.io/metadata.name": "ws-ws1"}},
            "podSelector": {"matchLabels": {"mandate.dev/archive-client": "true"}},
        }
    ]
    ingress = spec.get("ingress", [])
    if (
        len(ingress) != 1
        or ingress[0].get("from") != want_from
        or ingress[0].get("ports") != [{"protocol": "TCP", "port": 9000}]
    ):
        problems.append(f"the store accepts {ingress}")
    pod = deployment["spec"]["template"]["spec"]
    if (
        pod.get("automountServiceAccountToken") is not False
        or pod["securityContext"].get("runAsNonRoot") is not True
    ):
        problems.append("the store's pod is not non-root without a token")
    for container in [*pod.get("initContainers", []), *pod["containers"]]:
        context = container.get("securityContext", {})
        if not (
            context.get("readOnlyRootFilesystem") is True
            and context.get("allowPrivilegeEscalation") is False
            and context.get("capabilities") == {"drop": ["ALL"]}
            and container.get("resources", {}).get("limits")
        ):
            problems.append(f"{container['name']} is not hardened")
        for env in container.get("env", []):
            if "valueFrom" in env:
                problems.append(f"{container['name']} takes {env['name']} from a Secret")
    if client["metadata"].get("namespace") != "ws-ws1" or set(client.get("stringData", {})) != {
        "ACCESS_KEY_ID",
        "ACCESS_SECRET_KEY",
    }:
        problems.append("the journal's namespace does not get the archive's key")
    return problems


def plugin_tls(docs: list[dict]) -> list[str]:
    """The two Secrets the plugin and the operator read, each a self-signed TLS pair whose CA is
    itself, with when it ends recorded for renewal."""
    problems = []
    for name in ("barman-cloud-server-tls", "barman-cloud-client-tls"):
        secret = one(docs, "Secret", name)
        if secret is None:
            problems.append(f"no Secret {name}")
            continue
        data = secret.get("stringData", {})
        if secret.get("type") != "kubernetes.io/tls" or "PRIVATE KEY" not in data.get("tls.key", ""):
            problems.append(f"{name} is not a TLS pair")
        if not data.get("tls.crt") or data.get("ca.crt") != data.get("tls.crt"):
            problems.append(f"{name}'s CA is not its own certificate")
        if not secret["metadata"].get("annotations", {}).get("mandate.dev/not-after", "").isdigit():
            problems.append(f"{name} does not record when it ends")
    return problems


def test_standby_synchronous():
    assert standby_synchronous(journal()) == []


def test_archive_configured():
    assert archive_configured(journal()) == []


def test_egress_narrow():
    assert egress_narrow(journal()) == []


def test_failure_domain_disclosed_on_one_node_and_required_otherwise():
    assert failure_domain_disclosed(journal(), shared=True) == []
    assert failure_domain_disclosed(journal("--set", "failureDomain=separate"), shared=False) == []


def test_store_isolated():
    assert store_isolated(render(STORE_CHART, "archive-store")) == []


def test_plugin_tls():
    assert plugin_tls(render(TLS_CHART, "cnpg-system")) == []


def test_values_cannot_weaken_the_journal():
    """No value turns the standby off, opens a plain-HTTP archive outside the cluster, or leaves the
    archive out."""
    assert journal_refused("--set", "instances=1")
    assert journal_refused("--set", "synchronous.dataDurability=preferred")
    assert journal_refused("--set", "archive.endpointURL=")
    assert journal_refused("--set", "archive.endpointURL=http://store.example.com:9000")
    assert journal_refused(
        "--set",
        "archive.endpointURL=http://archive-store.archive-store.svc:9000",
        "--set",
        "archive.egress.namespace=",
        "--set-json",
        'archive.egress.cidrs=["203.0.113.0/24"]',
    )
    assert journal_refused("--set-json", 'archive.egress.cidrs=["0.0.0.0/0"]')
    assert journal_refused("--set-json", 'archive.egress.cidrs=["203.0.113.0/24"]')
    assert journal_refused("--set", "failureDomain=maybe")
    assert helm(JOURNAL_CHART, "ws-ws1", "-f", str(K3D_VALUES)).returncode != 0, (
        "a rendering with no cluster and no Kubernetes API address must fail, not open the API's port to all"
    )


def test_an_https_archive_by_cidr_renders():
    docs = journal(
        "--set",
        "archive.endpointURL=https://abc.r2.cloudflarestorage.com",
        "--set",
        "archive.egress.namespace=",
        "--set-json",
        "archive.egress.podSelector={}",
        "--set-json",
        'archive.egress.cidrs=["203.0.113.0/24"]',
        "--set",
        "archive.egress.port=443",
    )
    rules = one(docs, "NetworkPolicy", "journal")["spec"]["egress"]
    assert {
        "to": [{"ipBlock": {"cidr": "203.0.113.0/24"}}],
        "ports": [{"protocol": "TCP", "port": 443}],
    } in rules


def mutate(docs: list[dict], kind: str, name: str, change) -> list[dict]:
    docs = copy.deepcopy(docs)
    change(next(d for d in docs if d["kind"] == kind and d["metadata"]["name"] == name))
    return docs


def postgresql(c: dict) -> dict:
    return c["spec"]["postgresql"]


def journal_egress(p: dict) -> list:
    return p["spec"]["egress"]


JOURNAL_SEEDED = [
    (standby_synchronous, lambda d: mutate(d, "Cluster", "journal", lambda c: c["spec"].update(instances=1))),
    (
        standby_synchronous,
        lambda d: mutate(
            d, "Cluster", "journal", lambda c: postgresql(c)["synchronous"].update(dataDurability="preferred")
        ),
    ),
    (
        standby_synchronous,
        lambda d: mutate(d, "Cluster", "journal", lambda c: postgresql(c).pop("synchronous")),
    ),
    (
        standby_synchronous,
        lambda d: mutate(
            d, "Cluster", "journal", lambda c: postgresql(c)["parameters"].update(synchronous_commit="local")
        ),
    ),
    (archive_configured, lambda d: mutate(d, "Cluster", "journal", lambda c: c["spec"].pop("plugins"))),
    (
        archive_configured,
        lambda d: mutate(
            d,
            "ObjectStore",
            "journal-archive",
            lambda s: s["spec"]["configuration"]["s3Credentials"]["secretAccessKey"].update(
                name="journal-role-mandate-migrator"
            ),
        ),
    ),
    (archive_configured, lambda d: [x for x in d if x["kind"] != "ScheduledBackup"]),
    (egress_narrow, lambda d: mutate(d, "NetworkPolicy", "journal", lambda p: journal_egress(p).append({}))),
    (
        egress_narrow,
        lambda d: mutate(
            d,
            "NetworkPolicy",
            "journal",
            lambda p: journal_egress(p).append(
                {"to": [{"ipBlock": {"cidr": "0.0.0.0/0"}}], "ports": [{"port": 443}]}
            ),
        ),
    ),
    (egress_narrow, lambda d: mutate(d, "NetworkPolicy", "journal", lambda p: journal_egress(p).pop())),
    (
        egress_narrow,
        lambda d: mutate(d, "NetworkPolicy", "journal", lambda p: p["spec"].update(policyTypes=["Ingress"])),
    ),
]


@pytest.mark.parametrize(
    ("check", "seed"), JOURNAL_SEEDED, ids=[f"{c.__name__}-{i}" for i, (c, _) in enumerate(JOURNAL_SEEDED)]
)
def test_each_journal_seeded_bug_is_caught(check, seed):
    docs = journal()
    assert check(docs) == []
    assert check(seed(docs)), f"{check.__name__} missed its seeded bug"


def test_failure_domain_seeded_bugs_are_caught():
    shared = journal()
    hidden = mutate(
        shared,
        "Cluster",
        "journal",
        lambda c: c["metadata"]["annotations"].update({"mandate.dev/failure-domain": "fine"}),
    )
    assert failure_domain_disclosed(hidden, shared=True)
    separate = journal("--set", "failureDomain=separate")
    loose = mutate(
        separate,
        "Cluster",
        "journal",
        lambda c: c["spec"]["affinity"].update(podAntiAffinityType="preferred"),
    )
    assert failure_domain_disclosed(loose, shared=False)


def test_store_and_tls_seeded_bugs_are_caught():
    store = render(STORE_CHART, "archive-store")
    for change in (
        lambda d: mutate(d, "NetworkPolicy", "archive-store", lambda p: p["spec"].update(egress=[{}])),
        lambda d: mutate(
            d, "NetworkPolicy", "archive-store", lambda p: p["spec"]["ingress"][0].update({"from": [{}]})
        ),
        lambda d: mutate(
            d,
            "Deployment",
            "archive-store",
            lambda w: w["spec"]["template"]["spec"]["containers"][0]["securityContext"].update(
                readOnlyRootFilesystem=False
            ),
        ),
        lambda d: mutate(
            d,
            "Deployment",
            "archive-store",
            lambda w: w["spec"]["template"]["spec"]["containers"][0]["env"].append(
                {"name": "MINIO_ROOT_PASSWORD", "valueFrom": {"secretKeyRef": {"name": "k", "key": "p"}}}
            ),
        ),
        lambda d: [x for x in d if x["metadata"]["name"] != "journal-archive-key"],
    ):
        assert store_isolated(change(store)), change
    tls = render(TLS_CHART, "cnpg-system")
    for change in (
        lambda d: [x for x in d if x["metadata"]["name"] != "barman-cloud-client-tls"],
        lambda d: mutate(
            d, "Secret", "barman-cloud-server-tls", lambda s: s["stringData"].update({"ca.crt": "x"})
        ),
        lambda d: mutate(d, "Secret", "barman-cloud-server-tls", lambda s: s["metadata"].pop("annotations")),
    ):
        assert plugin_tls(change(tls)), change
