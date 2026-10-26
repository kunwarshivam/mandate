"""Invariants of the workspace vault's chart (E21-32, DEC-977, DEC-2019), on what it renders.

As in test_cell.py, each property is one function returning the problems it finds, asserted empty on
the real rendering and shown non-empty on a seeded bug. The expected settings are written out here
from the story and the decisions, not read from the chart, so a chart that drifts fails.
"""

import copy
import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

import pytest
from test_cell import PROCESSES, policies, render_all, workloads

CELL = Path(__file__).resolve().parents[1] / "cell"
CHART = CELL / "charts" / "vault"
MANAGED = CELL / "values" / "vault-managed.yaml"
STAGING = CELL / "values" / "vault-staging.yaml"
# Where the static seal reads its key: a file from the bootstrap-created Secret, never the
# environment (DEC-2021).
SEAL_KEY = "file:///openbao/seal/key"
NAMESPACE = "ws-ws1"
# The API server's addresses a render outside a cluster names, as test_gitops's do: staging reads
# them at install from the `kubernetes` Service and its EndpointSlice.
KUBE_API = {"cidrs": ["10.245.0.1/32", "10.110.0.2/32"], "ports": [443, 6443]}
# Staging's values, with the addresses its install would read.
STAGED = (
    "-f",
    str(CELL / "values" / "vault-staging.yaml"),
    "--set-json",
    f"kubeApi.cidrs={json.dumps(KUBE_API['cidrs'])}",
    "--set-json",
    f"kubeApi.ports={json.dumps(KUBE_API['ports'])}",
)
WORKSPACE = "ws1"
PORT = 8200
# The one command the server runs: server mode from the chart's config file, no dev flag (DEC-977
# item 6).
COMMAND = ["bao", "server", "-config=/openbao/config/config.hcl"]
DIGEST = re.compile(r"^[a-z0-9./_-]+(:[A-Za-z0-9._-]+)?@sha256:[0-9a-f]{64}$")
# What the server certificate must name: the Service, from the namespace and the cluster, and the
# pod's loopback.
NAMES = {
    "DNS:vault",
    f"DNS:vault.{NAMESPACE}",
    f"DNS:vault.{NAMESPACE}.svc",
    f"DNS:vault.{NAMESPACE}.svc.cluster.local",
    "DNS:localhost",
    "IP Address:127.0.0.1",
}

if shutil.which("openssl") is None:
    pytest.skip("openssl is not installed", allow_module_level=True)


def helm(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["helm", "template", "cell", str(CHART), "--namespace", NAMESPACE, *args],
        capture_output=True,
        text=True,
        check=False,
    )


def render(*args: str) -> list[dict]:
    done = helm(*args)
    assert done.returncode == 0, done.stderr
    lines = (line.strip() for line in done.stdout.splitlines())
    return [json.loads(line) for line in lines if line and line != "---" and not line.startswith("#")]


def one(docs: list[dict], kind: str, name: str) -> dict:
    found = [d for d in docs if d["kind"] == kind and d["metadata"]["name"] == name]
    assert len(found) == 1, f"{kind}/{name} rendered {len(found)} times"
    return found[0]


def server(docs: list[dict]) -> dict:
    return one(docs, "StatefulSet", "vault")


def container(docs: list[dict]) -> dict:
    (only,) = server(docs)["spec"]["template"]["spec"]["containers"]
    return only


def config(docs: list[dict]) -> str:
    return one(docs, "ConfigMap", "vault-config")["data"]["config.hcl"]


def blocks(hcl: str, kind: str) -> list[str]:
    """The labels of every `kind "label" {` block in `hcl`."""
    return re.findall(rf'^\s*{kind}\s+"([^"]+)"\s*\{{', hcl, re.MULTILINE)


def setting(hcl: str, key: str) -> list[str]:
    return re.findall(rf'^\s*{key}\s*=\s*"?([^"\n]*)"?\s*$', hcl, re.MULTILINE)


def root_generation_on_loopback_only(docs: list[dict]) -> list[str]:
    """Root generation from the recovery keys is answered only where they exist, staging's static
    seal, and only on a second listener bound to the pod's loopback (DEC-2630); the listener every
    process reaches keeps it off, as it is by default."""
    found = []
    hcl = config(docs)
    staged = blocks(hcl, "seal") == ["static"]
    addresses = setting(hcl, "address")
    opened = setting(hcl, "disable_unauthed_generate_root_endpoints")
    want = ["0.0.0.0:8200", "127.0.0.1:8210"] if staged else ["0.0.0.0:8200"]
    if addresses != want:
        found.append(f"the listeners bind {addresses}, not {want}")
    if opened != (["false"] if staged else []):
        found.append(f"root generation is {opened} with the seal {blocks(hcl, 'seal')}")
    return found


def server_mode(docs: list[dict]) -> list[str]:
    """Server mode on integrated storage, never the dev server: the one command, no argument and
    no environment variable that turns dev mode on, and raft storage alone (DEC-977 item 6)."""
    found = []
    c = container(docs)
    if c.get("command") != COMMAND or c.get("args"):
        found.append(f"the server runs {c.get('command')} {c.get('args')}, not {COMMAND}")
    for e in c.get("env", []):
        if "DEV" in e["name"]:
            found.append(f"the environment sets {e['name']}")
    hcl = config(docs)
    if blocks(hcl, "storage") != ["raft"]:
        found.append(f"the storage is {blocks(hcl, 'storage')}, not raft alone")
    if "inmem" in hcl or "dev" in "".join(setting(hcl, "storage")):
        found.append("the config names in-memory storage")
    return found


def tls_only(docs: list[dict]) -> list[str]:
    """One listener, TLS 1.3 from the chart's certificate, never disabled, and the pod's own client
    (the probe's and the operator's `bao`) speaking HTTPS and checking the certificate."""
    found = []
    hcl = config(docs)
    listeners = blocks(hcl, "listener")
    if listeners not in (["tcp"], ["tcp", "tcp"]):
        found.append(f"the listeners are {listeners}")
    if setting(hcl, "tls_disable"):
        found.append("a listener disables TLS")
    if setting(hcl, "tls_min_version") != ["tls13"] * len(listeners):
        found.append(f"the TLS floor is {setting(hcl, 'tls_min_version')}")
    certs = setting(hcl, "tls_cert_file"), setting(hcl, "tls_key_file")
    if certs != (["/openbao/tls/tls.crt"] * len(listeners), ["/openbao/tls/tls.key"] * len(listeners)):
        found.append("a listener does not use the chart's certificate")
    env = {e["name"]: e.get("value") for e in container(docs).get("env", [])}
    if not str(env.get("BAO_ADDR")).startswith("https://") or env.get("BAO_CACERT") != "/openbao/tls/ca.crt":
        found.append(f"the pod's own client speaks to {env.get('BAO_ADDR')} trusting {env.get('BAO_CACERT')}")
    if any(k in env for k in ("BAO_SKIP_VERIFY", "VAULT_SKIP_VERIFY", "BAO_TLS_SERVER_NAME")):
        found.append("the pod's own client skips the server's certificate")
    return found


def sealed_is_not_ready(docs: list[dict]) -> list[str]:
    """A sealed or uninitialized vault is not ready, so the Service routes nothing to it, and
    nothing restarts it on its own, since a restart seals it again (DEC-977 item 5)."""
    found = []
    c = container(docs)
    if c.get("readinessProbe", {}).get("exec", {}).get("command") != ["bao", "status"]:
        found.append(f"the readiness probe is {c.get('readinessProbe')}, which a sealed vault may pass")
    if c.get("livenessProbe") or c.get("startupProbe"):
        found.append("a probe restarts the vault, which seals it")
    if one(docs, "Service", "vault")["spec"].get("publishNotReadyAddresses"):
        found.append("the Service routes to a sealed vault")
    return found


def reached_only_by_its_processes(docs: list[dict], api: dict | None = None) -> list[str]:
    """Only the workspace's pods whose egress names the vault reach it, on 8200; it reaches
    nothing but, with Kubernetes auth (`api`, its CIDRs and ports), the cluster's API server for
    TokenReview, by address and port alone."""
    found = []
    policy = one(docs, "NetworkPolicy", "vault")["spec"]
    labels = server(docs)["spec"]["template"]["metadata"]["labels"]
    if any(labels.get(k) != v for k, v in policy["podSelector"]["matchLabels"].items()):
        found.append("the policy does not select the vault's pods")
    want_egress = (
        [
            {
                "to": [{"ipBlock": {"cidr": cidr}} for cidr in api["cidrs"]],
                "ports": [{"protocol": "TCP", "port": port} for port in api["ports"]],
            }
        ]
        if api
        else []
    )
    if sorted(policy.get("policyTypes", [])) != ["Egress", "Ingress"] or (policy.get("egress") or []) != want_egress:
        found.append(f"the vault's egress is {policy.get('egress')}")
    want_from = [
        {
            "podSelector": {
                "matchLabels": {"mandate.dev/workspace": WORKSPACE, "mandate.dev/egress-vault": "true"}
            }
        }
    ]
    want_ports = [{"protocol": "TCP", "port": PORT}]
    rules = policy.get("ingress", [])
    if len(rules) != 1 or rules[0].get("from") != want_from or rules[0].get("ports") != want_ports:
        found.append(f"the vault accepts {rules}")
    service = one(docs, "Service", "vault")["spec"]
    if service.get("type", "ClusterIP") != "ClusterIP" or [p["port"] for p in service["ports"]] != [PORT]:
        found.append(f"the Service exposes {service.get('type')} {service['ports']}")
    return found


def hardened(docs: list[dict]) -> list[str]:
    """Non-root, a read-only root filesystem, no capability, no escalation, the runtime's seccomp
    profile, no service-account token, requests equal to limits, and the image by digest."""
    found = []
    spec = server(docs)["spec"]["template"]["spec"]
    c = container(docs)
    pod_sc = spec.get("securityContext", {})
    sc = c.get("securityContext", {})
    if spec.get("automountServiceAccountToken") is not False:
        found.append("the pod mounts a service-account token")
    if (
        pod_sc.get("runAsNonRoot") is not True
        or pod_sc.get("seccompProfile", {}).get("type") != "RuntimeDefault"
    ):
        found.append(f"the pod runs as {pod_sc}")
    if sc.get("allowPrivilegeEscalation") is not False or sc.get("readOnlyRootFilesystem") is not True:
        found.append(f"the container runs as {sc}")
    if sc.get("capabilities", {}).get("drop") != ["ALL"] or sc.get("capabilities", {}).get("add"):
        found.append(f"the container's capabilities are {sc.get('capabilities')}")
    if spec.get("hostNetwork") or spec.get("hostPID") or sc.get("privileged"):
        found.append("the pod shares the host's")
    resources = c.get("resources", {})
    if not resources.get("limits") or resources.get("requests") != resources.get("limits"):
        found.append(f"the container's resources are {resources}")
    if not DIGEST.match(c["image"]):
        found.append(f"the image {c['image']} is not pinned by digest")
    return found


def secrets_as_files(docs: list[dict]) -> list[str]:
    """No secret in the environment: the only Secret is the server's TLS pair, mounted read-only,
    and the CA's key is in nothing the chart renders (DEC-822 item 4)."""
    found = []
    c = container(docs)
    if c.get("envFrom") or any("valueFrom" in e for e in c.get("env", [])):
        found.append("the environment reads a Secret")
    secrets = [d["metadata"]["name"] for d in docs if d["kind"] == "Secret"]
    if secrets != ["vault-tls"]:
        found.append(f"the chart renders the Secrets {secrets}")
    tls = one(docs, "Secret", "vault-tls")["stringData"]
    if sorted(tls) != ["ca.crt", "tls.crt", "tls.key"]:
        found.append(f"the TLS Secret holds {sorted(tls)}")
    ca = one(docs, "ConfigMap", "vault-ca")["data"]
    if sorted(ca) != ["ca.crt"] or "PRIVATE KEY" in ca["ca.crt"]:
        found.append("the CA's ConfigMap holds more than its certificate")
    if sum(json.dumps(d).count("PRIVATE KEY-----") for d in docs) != 2:
        found.append("a private key other than the server's is rendered")
    return found


def certificate(docs: list[dict]) -> list[str]:
    """The server certificate names the Service and the loopback and is signed by the CA the
    clients are given."""
    found = []
    tls = one(docs, "Secret", "vault-tls")["stringData"]
    with tempfile.TemporaryDirectory() as tmp:
        crt, ca = Path(tmp) / "tls.crt", Path(tmp) / "ca.crt"
        crt.write_text(tls["tls.crt"])
        ca.write_text(one(docs, "ConfigMap", "vault-ca")["data"]["ca.crt"])
        verified = subprocess.run(
            ["openssl", "verify", "-CAfile", str(ca), str(crt)], capture_output=True, text=True, check=False
        )
        if verified.returncode != 0:
            found.append(f"the CA does not sign the server certificate: {verified.stderr.strip()}")
        sans = subprocess.run(
            ["openssl", "x509", "-noout", "-ext", "subjectAltName", "-in", str(crt)],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
    named = {n.strip() for n in sans.splitlines()[-1].split(",")}
    if named != NAMES:
        found.append(f"the certificate names {sorted(named)}")
    return found


def seal_volume(docs: list[dict]) -> tuple[list[dict], list[dict]]:
    spec = server(docs)["spec"]["template"]["spec"]
    volumes = [v for v in spec["volumes"] if v["name"] == "seal"]
    mounts = [m for m in container(docs)["volumeMounts"] if m["name"] == "seal" or "seal" in m["mountPath"]]
    return volumes, mounts


def sealed_by_hand(docs: list[dict]) -> list[str]:
    """Outside staging the vault unseals by hand: no seal block, no seal key mounted (DEC-977
    item 5, DEC-2021)."""
    found = []
    if blocks(config(docs), "seal"):
        found.append(f"the config has the seal blocks {blocks(config(docs), 'seal')}")
    if any(seal_volume(docs)):
        found.append("a seal key is mounted")
    return found


def sealed_by_staging_key(docs: list[dict]) -> list[str]:
    """Staging unseals itself with the static seal, its key a file from the Secret the bootstrap
    makes, mounted read-only, and nothing else (DEC-2605 row 10, DEC-2021)."""
    found = []
    hcl = config(docs)
    if blocks(hcl, "seal") != ["static"] or setting(hcl, "current_key") != [SEAL_KEY]:
        found.append(f"the seal is {blocks(hcl, 'seal')} from {setting(hcl, 'current_key')}")
    if setting(hcl, "previous_key") or not all(setting(hcl, "current_key_id")):
        found.append("the seal names no key id, or a second key")
    volumes, mounts = seal_volume(docs)
    want_volume = [
        {
            "name": "seal",
            "secret": {
                "secretName": "vault-seal-key",
                "defaultMode": 0o440,
                "items": [{"key": "key", "path": "key"}],
            },
        }
    ]
    if volumes != want_volume:
        found.append(f"the seal volume is {volumes}")
    if mounts != [{"name": "seal", "mountPath": "/openbao/seal", "readOnly": True}]:
        found.append(f"the seal key is mounted as {mounts}")
    return found


# Where OpenBao reads the in-cluster token and CA it reviews other tokens with (DEC-2062).
TOKEN_PATH = "/var/run/secrets/kubernetes.io/serviceaccount"
REVIEWER = {
    "apiGroup": "rbac.authorization.k8s.io",
    "kind": "ClusterRole",
    "name": "system:auth-delegator",
}


def token_review(docs: list[dict], enabled: bool) -> list[str]:
    """The vault runs as its own ServiceAccount, which mounts no token by itself (DEC-2062). With
    Kubernetes auth on, and only then: one projected volume, read by owner and group, holds a token of that
    account for the API server's default audience, lasting an hour at most, and the cluster's CA,
    at the path OpenBao reads; and one binding gives that account, in its namespace, the
    `system:auth-delegator` role and nothing else. No other role, binding or token Secret. The
    pod names its user and its fsGroup and no container overrides them, since the kubelet makes
    the projected files readable through exactly those."""
    found = []
    spec = server(docs)["spec"]["template"]["spec"]
    pod_sc = spec.get("securityContext", {})
    if not pod_sc.get("runAsUser") or not pod_sc.get("fsGroup"):
        found.append(f"the pod names no user or fsGroup: {pod_sc}")
    for c in spec.get("containers", []) + spec.get("initContainers", []):
        sc = c.get("securityContext", {})
        if any(k in sc for k in ("runAsUser", "runAsGroup")):
            found.append(f"{c['name']} runs as {sc}, not the pod's user")
    accounts = [d for d in docs if d["kind"] == "ServiceAccount"]
    if [a["metadata"]["name"] for a in accounts] != ["vault"] or any(
        a.get("automountServiceAccountToken") is not False for a in accounts
    ):
        found.append(f"the ServiceAccounts are {accounts}")
    if spec.get("serviceAccountName") != "vault" or spec.get("automountServiceAccountToken") is not False:
        who, automount = spec.get("serviceAccountName"), spec.get("automountServiceAccountToken")
        found.append(f"the pod runs as {who}, automount {automount}")
    for d in docs:
        if d["kind"] in ("Role", "RoleBinding", "ClusterRole") or (
            d["kind"] == "Secret" and d.get("type") == "kubernetes.io/service-account-token"
        ):
            found.append(f"{d['kind']} {d['metadata']['name']} is rendered")
    bindings = [d for d in docs if d["kind"] == "ClusterRoleBinding"]
    projected = [v for v in spec.get("volumes", []) if "projected" in v]
    mounts = [m for m in container(docs).get("volumeMounts", []) if m["mountPath"] == TOKEN_PATH]
    if not enabled:
        if bindings or projected or mounts:
            found.append(f"Kubernetes auth is off, yet {bindings} {projected} {mounts}")
        return found
    subject = [{"kind": "ServiceAccount", "name": "vault", "namespace": NAMESPACE}]
    if len(bindings) != 1 or bindings[0].get("roleRef") != REVIEWER or bindings[0].get("subjects") != subject:
        found.append(f"the review bindings are {bindings}")
    want = [
        {"serviceAccountToken": {"path": "token", "expirationSeconds": 3600}},
        {"configMap": {"name": "kube-root-ca.crt", "items": [{"key": "ca.crt", "path": "ca.crt"}]}},
    ]
    if len(projected) != 1 or projected[0]["projected"] != {"defaultMode": 0o440, "sources": want}:
        found.append(f"the projected volumes are {projected}")
    if (
        len(mounts) != 1
        or mounts[0].get("readOnly") is not True
        or (projected and mounts[0]["name"] != projected[0]["name"])
    ):
        found.append(f"the token is mounted as {mounts}")
    if "token_reviewer" in json.dumps(docs):
        found.append("a reviewer token is stored rather than reread")
    return found


def every(docs: list[dict], api: dict | None = None) -> list[str]:
    """Every invariant, with Kubernetes auth on exactly when `api` names the API server."""
    return [
        *server_mode(docs),
        *tls_only(docs),
        *sealed_is_not_ready(docs),
        *reached_only_by_its_processes(docs, api),
        *root_generation_on_loopback_only(docs),
        *hardened(docs),
        *secrets_as_files(docs),
        *certificate(docs),
        *token_review(docs, enabled=api is not None),
    ]


def test_every_invariant_holds_on_each_rendering():
    for docs, api in ((render(), None), (render("-f", str(MANAGED)), None), (render(*STAGED), KUBE_API)):
        assert every(docs, api) == []


def test_the_cell_and_production_unseal_by_hand_and_staging_by_its_key():
    assert sealed_by_hand(render()) == []
    assert sealed_by_hand(render("-f", str(MANAGED))) == []
    assert sealed_by_staging_key(render(*STAGED)) == []


def test_the_static_seal_is_refused_outside_staging():
    static = ("--set", "seal.type=static", "--set", "seal.keyId=k1")
    assert helm(*static).returncode != 0
    assert helm("-f", str(MANAGED), *static).returncode != 0
    assert helm("--set", "environment=staging", "--set", "seal.type=static").returncode != 0
    assert helm("--set", "seal.type=awskms").returncode != 0
    assert helm("--set", "environment=staging", *static).returncode == 0


def test_the_seal_checks_catch_a_key_in_the_wrong_place():
    hand = render()
    assert sealed_by_hand(with_config(hand, 'storage "raft"', 'seal "static" {}\n    storage "raft"'))
    staged = render(*STAGED)
    assert sealed_by_staging_key(with_config(staged, SEAL_KEY, "env://BAO_SEAL_KEY"))
    assert sealed_by_staging_key(
        with_config(staged, 'current_key = "', 'previous_key = "x"\n      current_key = "')
    )
    assert sealed_by_hand(staged)
    bad = copy.deepcopy(staged)
    mount = next(m for m in container(bad)["volumeMounts"] if m["name"] == "seal")
    mount["readOnly"] = False
    assert sealed_by_staging_key(bad)
    bad = copy.deepcopy(staged)
    volume = next(v for v in server(bad)["spec"]["template"]["spec"]["volumes"] if v["name"] == "seal")
    volume["secret"]["defaultMode"] = 0o444
    assert sealed_by_staging_key(bad)


def test_the_managed_values_change_only_the_volume():
    for values in (("-f", str(MANAGED)), STAGED):
        claim = server(render(*values))["spec"]["volumeClaimTemplates"][0]["spec"]
        assert claim["storageClassName"] == "do-block-storage"
    assert "storageClassName" not in server(render())["spec"]["volumeClaimTemplates"][0]["spec"]


def with_container(docs: list[dict], **changes) -> list[dict]:
    bad = copy.deepcopy(docs)
    container(bad).update(changes)
    return bad


def with_config(docs: list[dict], old: str, new: str) -> list[dict]:
    bad = copy.deepcopy(docs)
    data = one(bad, "ConfigMap", "vault-config")["data"]
    assert old in data["config.hcl"]
    data["config.hcl"] = data["config.hcl"].replace(old, new)
    return bad


def test_root_generation_catches_it_on_the_shared_listener_or_outside_staging():
    staged = render(*STAGED)
    assert root_generation_on_loopback_only(staged) == []
    root_on = "disable_unauthed_generate_root_endpoints = false"
    shared = with_config(staged, 'cluster_address = "127.0.0.1:8201"', f'cluster_address = "127.0.0.1:8201"\n      {root_on}')
    assert root_generation_on_loopback_only(shared)
    widened = with_config(staged, 'address = "127.0.0.1:8210"', 'address = "0.0.0.0:8210"')
    assert root_generation_on_loopback_only(widened)
    loopback = f'listener "tcp" {{\n      address = "127.0.0.1:8210"\n      {root_on}\n    }}\n    storage "raft"'
    assert root_generation_on_loopback_only(with_config(render(), 'storage "raft"', loopback))


def test_server_mode_catches_the_dev_server():
    docs = render()
    assert server_mode(with_container(docs, args=["-dev"]))
    assert server_mode(with_container(docs, command=["bao", "server", "-dev", "-dev-tls"]))
    assert server_mode(with_container(docs, env=[{"name": "BAO_DEV_ROOT_TOKEN_ID", "value": "x"}]))
    assert server_mode(with_config(docs, 'storage "raft"', 'storage "inmem"'))


def test_tls_only_catches_plain_http():
    docs = render()
    assert tls_only(with_config(docs, "tls_min_version", 'tls_disable = "true"\n      tls_min_version'))
    assert tls_only(with_config(docs, '"tls13"', '"tls12"'))
    assert tls_only(with_container(docs, env=[{"name": "BAO_ADDR", "value": "http://127.0.0.1:8200"}]))
    env = [*container(docs)["env"], {"name": "BAO_SKIP_VERIFY", "value": "true"}]
    assert tls_only(with_container(docs, env=env))


def test_sealed_is_not_ready_catches_a_restart_or_a_sealed_route():
    docs = render()
    assert sealed_is_not_ready(with_container(docs, livenessProbe={"tcpSocket": {"port": PORT}}))
    assert sealed_is_not_ready(
        with_container(docs, readinessProbe={"exec": {"command": ["sh", "-c", "bao status || true"]}})
    )
    bad = copy.deepcopy(docs)
    one(bad, "Service", "vault")["spec"]["publishNotReadyAddresses"] = True
    assert sealed_is_not_ready(bad)


def test_kubernetes_auth_reaches_only_the_api_server():
    plain = render(*STAGED, "--set", "kubeApi.enabled=false")
    assert reached_only_by_its_processes(plain) == []
    on = render("-f", str(STAGING), "--set-json", f"kubeApi={json.dumps({'enabled': True, **KUBE_API})}")
    assert reached_only_by_its_processes(on, KUBE_API) == []
    assert reached_only_by_its_processes(on), "the plain check must refuse the API rule"
    for mutate in (
        lambda e: e[0]["to"].append({"ipBlock": {"cidr": "0.0.0.0/0"}}),
        lambda e: e[0]["ports"].append({"protocol": "TCP", "port": 8443}),
        lambda e: e[0].pop("ports"),
        lambda e: e[0]["to"].append({"namespaceSelector": {}}),
        lambda e: e.append({"to": [{"podSelector": {}}]}),
    ):
        bad = copy.deepcopy(on)
        mutate(one(bad, "NetworkPolicy", "vault")["spec"]["egress"])
        assert reached_only_by_its_processes(bad, KUBE_API), mutate
    unknown = helm("-f", str(STAGING), "--set", "kubeApi.enabled=true")
    assert unknown.returncode != 0 and "Kubernetes API" in unknown.stderr
    for cidrs in (["fd00::1/32"], ["10.245.0.1/32", "fd00::1/32"], ["10.0.0.0/8"], ["0.0.0.0/0"], ["10.245.0.1"]):
        api = json.dumps({"enabled": True, "cidrs": cidrs, "ports": [443]})
        assert helm("-f", str(STAGING), "--set-json", f"kubeApi={api}").returncode != 0, cidrs
        skipped = helm("-f", str(STAGING), "--skip-schema-validation", "--set-json", f"kubeApi={api}")
        assert skipped.returncode != 0 and "Kubernetes API" in skipped.stderr, cidrs


def test_reached_only_by_its_processes_catches_a_wider_policy():
    docs = render()
    for mutate in (
        lambda p: p["ingress"][0]["from"].append({"namespaceSelector": {}}),
        lambda p: p["ingress"][0]["from"][0]["podSelector"]["matchLabels"].pop("mandate.dev/egress-vault"),
        lambda p: p["ingress"][0]["ports"].append({"protocol": "TCP", "port": 8201}),
        lambda p: p.update(egress=[{}]),
        lambda p: p.update(policyTypes=["Ingress"]),
    ):
        bad = copy.deepcopy(docs)
        mutate(one(bad, "NetworkPolicy", "vault")["spec"])
        assert reached_only_by_its_processes(bad)
    bad = copy.deepcopy(docs)
    one(bad, "Service", "vault")["spec"]["type"] = "NodePort"
    assert reached_only_by_its_processes(bad)


def test_hardened_catches_each_weakening():
    docs = render()
    assert hardened(with_container(docs, image="ghcr.io/openbao/openbao:2.7.1"))
    assert hardened(
        with_container(docs, securityContext={"capabilities": {"add": ["IPC_LOCK"], "drop": ["ALL"]}})
    )
    assert hardened(with_container(docs, resources={"limits": {"cpu": "1"}}))
    bad = copy.deepcopy(docs)
    server(bad)["spec"]["template"]["spec"]["automountServiceAccountToken"] = True
    assert hardened(bad)


def test_secrets_as_files_catches_a_secret_in_the_environment_or_a_kept_ca_key():
    docs = render()
    assert secrets_as_files(
        with_container(
            docs, env=[{"name": "BAO_TOKEN", "valueFrom": {"secretKeyRef": {"name": "x", "key": "t"}}}]
        )
    )
    bad = copy.deepcopy(docs)
    one(bad, "Secret", "vault-tls")["stringData"]["ca.key"] = "-----BEGIN RSA PRIVATE KEY-----"
    assert secrets_as_files(bad)


def test_certificate_catches_a_foreign_ca():
    docs = render()
    other = render()
    bad = copy.deepcopy(docs)
    one(bad, "ConfigMap", "vault-ca")["data"]["ca.crt"] = one(other, "ConfigMap", "vault-ca")["data"][
        "ca.crt"
    ]
    assert certificate(bad)


def test_values_refuse_a_secret_a_tag_or_a_server_argument():
    assert helm("--set", "rootToken=x").returncode != 0
    assert helm("--set", "unsealKey=x").returncode != 0
    assert helm("--set-json", 'args=["-dev"]').returncode != 0
    assert helm("--set", "image=ghcr.io/openbao/openbao:2.7.1").returncode != 0


def test_the_workspace_reaches_the_vault_it_selects():
    """The workspace chart's vault egress selects this chart's pods on its port, and every workspace
    pod that may reach the vault carries the label the vault's ingress admits (cross-chart trace)."""
    vault = render()
    pod_labels = server(vault)["spec"]["template"]["metadata"]["labels"]
    admitted = one(vault, "NetworkPolicy", "vault")["spec"]["ingress"][0]["from"][0]["podSelector"][
        "matchLabels"
    ]
    workspace = render_all()
    reaching = []
    for name, policy in policies(workspace).items():
        for rule in policy["spec"].get("egress", []):
            for to in rule.get("to", []):
                selected = to.get("podSelector", {}).get("matchLabels", {})
                if selected.get("mandate.dev/process") != "vault":
                    continue
                reaching.append(name)
                assert all(pod_labels.get(k) == v for k, v in selected.items()), (name, selected)
                assert [p["port"] for p in rule["ports"]] == [PORT], (name, rule["ports"])
    assert reaching, "no workspace process reaches the vault"
    labelled = set()
    for w in workloads(workspace):
        labels = w["spec"]["template"]["metadata"]["labels"]
        if labels.get("mandate.dev/egress-vault") == "true":
            assert all(labels.get(k) == v for k, v in admitted.items()), labels
            labelled.add(labels["mandate.dev/process"])
    declared = {p["name"] for p in PROCESSES if "vault" in p["egress"]}
    assert declared, "no process in processes.json names the vault"
    assert labelled == declared, f"admitted {sorted(labelled)}, declared {sorted(declared)}"


def test_the_vault_reviews_tokens_only_with_kubernetes_auth_on():
    on = render("-f", str(STAGING), "--set-json", f"kubeApi={json.dumps({'enabled': True, **KUBE_API})}")
    assert token_review(on, enabled=True) == []
    assert token_review(on, enabled=False), "the off check must refuse the binding"
    assert token_review(render(*STAGED), enabled=True) == []
    assert token_review(render(*STAGED, "--set", "kubeApi.enabled=false"), enabled=False) == []
    statefulset = ("StatefulSet", "vault")
    binding = ("ClusterRoleBinding", f"vault-token-review-{NAMESPACE}")
    account = ("ServiceAccount", "vault")

    def volume(d: dict) -> dict:
        (v,) = [v for v in d["spec"]["template"]["spec"]["volumes"] if "projected" in v]
        return v["projected"]

    for where, mutate in (
        (statefulset, lambda d: d["spec"]["template"]["spec"].update(automountServiceAccountToken=True)),
        (statefulset, lambda d: d["spec"]["template"]["spec"].pop("serviceAccountName")),
        (account, lambda d: d.pop("automountServiceAccountToken")),
        (binding, lambda d: d["roleRef"].update(name="cluster-admin")),
        (binding, lambda d: d["roleRef"].update(name="system:auth-delegator-wide")),
        (binding, lambda d: d["subjects"].append({"kind": "Group", "name": "system:authenticated"})),
        (binding, lambda d: d["subjects"][0].update(namespace="default")),
        (binding, lambda d: d["subjects"][0].update(name="default")),
        (
            statefulset,
            lambda d: volume(d)["sources"][0]["serviceAccountToken"].update(expirationSeconds=86400),
        ),
        (
            statefulset,
            lambda d: volume(d)["sources"][0]["serviceAccountToken"].update(audience="mandate-vault"),
        ),
        (statefulset, lambda d: volume(d).update(defaultMode=0o444)),
        (statefulset, lambda d: volume(d).update(defaultMode=0o400)),
        (statefulset, lambda d: d["spec"]["template"]["spec"]["securityContext"].pop("runAsUser")),
        (statefulset, lambda d: d["spec"]["template"]["spec"]["securityContext"].pop("fsGroup")),
        (statefulset, lambda d: d["spec"]["template"]["spec"]["containers"][0]["securityContext"].update(runAsUser=0)),
        (statefulset, lambda d: volume(d)["sources"].pop(1)),
        (statefulset, lambda d: volume(d)["sources"].append({"secret": {"name": "vault-tls"}})),
        (
            statefulset,
            lambda d: d["spec"]["template"]["spec"]["containers"][0]["volumeMounts"][-1].update(
                readOnly=False
            ),
        ),
    ):
        bad = copy.deepcopy(on)
        mutate(one(bad, *where))
        assert token_review(bad, enabled=True), mutate
    for added in (
        {"kind": "ClusterRoleBinding", "metadata": {"name": "second"}, "roleRef": REVIEWER, "subjects": []},
        {"kind": "RoleBinding", "metadata": {"name": "extra"}, "roleRef": {"kind": "Role", "name": "x"}},
        {"kind": "Secret", "metadata": {"name": "t"}, "type": "kubernetes.io/service-account-token"},
        {"kind": "ServiceAccount", "metadata": {"name": "other"}, "automountServiceAccountToken": False},
    ):
        assert token_review([*on, added], enabled=True), added
