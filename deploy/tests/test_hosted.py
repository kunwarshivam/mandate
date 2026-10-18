"""Invariants of cell #1's hosted bring-up (E21-51, DEC-2001), on the committed files and on what
deploy/hosted/cell1.py would run.

Each property is one function returning the problems it finds, asserted empty on the real files and
shown non-empty on a seeded bug. The bring-up itself is run twice with every external tool replaced
by a recorder: once as a dry run, which must call nothing, and once for real against recorders and a
fake Cloudflare API, which must put each secret only on a process's stdin.
"""

import base64
import copy
import importlib.util
import io
import json
import os
import re
import shutil
import stat
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
HOSTED = ROOT / "deploy" / "hosted"
SPEC = importlib.util.spec_from_file_location("cell1", HOSTED / "cell1.py")
cell1 = importlib.util.module_from_spec(SPEC)
sys.modules["cell1"] = cell1
SPEC.loader.exec_module(cell1)
CONFIG = json.loads((HOSTED / "cell1.json").read_text())
FIREWALL = json.loads((HOSTED / "firewall.json").read_text())
XTASK_CELL = (ROOT / "xtask" / "src" / "cell.rs").read_text()
JOURNAL_CHART = ROOT / "deploy" / "cell" / "charts" / "journal-db"
LOCAL_JOURNAL_VALUES = ROOT / "deploy" / "cell" / "values" / "k3d-journal-db.yaml"
# The real helm, found before a test puts the recorders first on PATH.
HELM = shutil.which("helm")
KEY = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl founder@laptop"
TOOLS = ("doctl", "cloudflared", "ssh", "kubectl", "helm", "curl", "k3d")
# Values planted in the secret files of the recorded run; none may reach argv or output.
SECRETS = {
    "cf": "cf-api-token-0123456789abcdef",
    "ghcr": "ghp_readpackages0123456789abcdef",
    "r2-id": "r2-access-key-id-0123456789",
    "r2-key": "r2-secret-access-key-0123456789abcdef",
    "archive-id": "archive-access-key-id-0123456789",
    "archive-key": "archive-secret-access-key-0123456789abcdef",
}
OPS_TOKEN = "ops-tunnel-token-0123456789abcdef"
APP_TOKEN = "app-tunnel-token-0123456789abcdef"
JOIN_TOKEN = "k3s-join-token-0123456789abcdef"


def user_data(role: str, token: str = OPS_TOKEN) -> dict:
    text = cell1.render_user_data(role, KEY, f"{role}-data", token if role == "server" else "", "10.110.0.2")
    header, body = text.split("\n", 1)
    assert header == "#cloud-config"
    return json.loads(body)


def files(document: dict) -> dict[str, dict]:
    return {f["path"]: f for f in document["write_files"]}


def no_public_inbound(firewall: dict) -> list[str]:
    """Nothing on the internet reaches a droplet: every inbound rule's only source is the cell's tag."""
    problems = []
    for rule in firewall["inbound"]:
        sources = rule["sources"]
        if set(sources) != {"tags"} or sources["tags"] != [CONFIG["tag"]]:
            problems.append(f"inbound {rule['protocol']} {rule.get('ports', '')} from {sources}")
    rendered = cell1.firewall_rules("inbound") if firewall is FIREWALL else ""
    if "address:" in rendered:
        problems.append("doctl's inbound rules name an address")
    return problems


def outbound_by_port(firewall: dict) -> list[str]:
    """Outbound to the internet only on the ports firewall.json explains: never every port."""
    allowed = {("tcp", "443"), ("tcp", "80"), ("udp", "53"), ("tcp", "53"), ("udp", "123")}
    allowed |= {("udp", "7844"), ("tcp", "7844")}
    problems = []
    for rule in firewall["outbound"]:
        if "addresses" in rule["destinations"]:
            if (rule["protocol"], rule.get("ports")) not in allowed or not rule.get("why"):
                problems.append(f"outbound {rule['protocol']} {rule.get('ports')} to the internet")
        elif rule["destinations"].get("tags") != [CONFIG["tag"]]:
            problems.append(f"outbound to {rule['destinations']}")
    return problems


def downloads_checked(document: dict) -> list[str]:
    """Every binary cloud-init fetches is checked against cell1.json's SHA-256 before it is
    installed, one command a line, so `set -e` stops at a mismatch."""
    script = files(document)["/usr/local/sbin/cell1-firstboot"]["content"]
    lines = script.splitlines()
    problems = []
    if "set -eu" not in lines:
        problems.append("the first-boot script does not stop at a failure")
    for line in lines:
        if "&&" in line or "||" in line:
            problems.append(f"a chained command hides its failure from set -e: {line}")
    for i, line in enumerate(lines):
        match = re.match(r"curl .* -o (\S+) '(\S+)'$", line)
        if not match:
            continue
        tmp, url = match.groups()
        want = next(c for c in (CONFIG["k3s"], CONFIG["cloudflared"]) if c["url"] == url)
        check = f"echo '{want['sha256']}  {tmp}' | sha256sum --check --strict"
        if lines[i + 1 : i + 2] != [check] or not lines[i + 2].startswith(f"install -m 0755 {tmp} "):
            problems.append(f"{url} is installed without its checksum")
    if not any("k3s/releases/download" in line for line in lines):
        problems.append("k3s is not fetched")
    return problems


def nothing_starts_k3s(document: dict) -> list[str]:
    """cloud-init installs k3s but never starts or enables it; `cell1.py nodes` does, once the
    node's config and join token are in place."""
    script = files(document)["/usr/local/sbin/cell1-firstboot"]["content"]
    return [line for line in script.splitlines() if re.search(r"(enable|start).*k3s|cell1-k3s-start", line)]


def ops_token_held(server: dict, agent: dict) -> list[str]:
    """The ops tunnel's token is on the server only, as a root-only 0600 file the tunnel's unit
    loads as a credential; the agent's user data holds no secret."""
    problems = []
    entry = files(server).get("/etc/cell1/credentials/ops-tunnel-token")
    if entry is None or entry["permissions"] != "0600" or entry.get("owner") != "root:root":
        problems.append(
            f"the ops token's file is {entry and {k: entry.get(k) for k in ('permissions', 'owner')}}"
        )
    unit = files(server)["/etc/systemd/system/cloudflared-ops.service"]["content"]
    if "LoadCredential=tunnel-token:/etc/cell1/credentials/ops-tunnel-token" not in unit:
        problems.append("the ops tunnel does not load its token as a credential")
    if "--token-file ${CREDENTIALS_DIRECTORY}/tunnel-token" not in unit or re.search(r"--token\s", unit):
        problems.append("the ops tunnel does not read its token from the credential")
    if OPS_TOKEN in json.dumps(agent):
        problems.append("the agent's user data holds the ops token")
    if any("credentials" in path for path in files(agent)):
        problems.append("the agent's user data writes a credential")
    return problems


def ssh_hardened(document: dict) -> list[str]:
    conf = files(document)["/etc/ssh/sshd_config.d/10-cell1.conf"]["content"]
    want = {
        "PermitRootLogin no",
        "PasswordAuthentication no",
        "KbdInteractiveAuthentication no",
        f"AllowUsers {CONFIG['admin_user']}",
    }
    problems = [f"sshd lacks `{w}`" for w in sorted(want - set(conf.splitlines()))]
    if not document.get("disable_root") or document.get("ssh_pwauth") is not False:
        problems.append("root login or password login is not disabled")
    user = document["users"][0]
    if user["ssh_authorized_keys"] != [KEY] or not user.get("lock_passwd"):
        problems.append("the admin user has a password or another key")
    return problems


def pods_kept_from_metadata(document: dict) -> list[str]:
    """No pod reaches the metadata service, which serves the user data, before k3s first starts."""
    block = files(document)["/usr/local/sbin/cell1-metadata-block"]["content"]
    unit = files(document)["/etc/systemd/system/k3s.service"]["content"]
    problems = []
    if "-t raw" not in block or "-i cni0 -d 169.254.169.254/32 -j DROP" not in block:
        problems.append("the metadata block is not a raw PREROUTING drop from the pod bridge")
    if "Requires=cell1-metadata-block.service" not in unit or "After=" not in unit:
        problems.append("k3s can start without the metadata block")
    return problems


def pins_match_xtask() -> list[str]:
    """cell #1 runs the k3s and CloudNativePG the local cell's drills run (xtask/src/cell.rs)."""
    problems = []
    image = re.search(r'const K3S_IMAGE: &str = "rancher/k3s:(v[^@"]+)@', XTASK_CELL).group(1)
    if (
        image.replace("-k3s", "+k3s") != CONFIG["k3s"]["version"]
        or CONFIG["k3d"]["image"].split(":")[1].split("@")[0] != image
    ):
        problems.append(f"k3s {CONFIG['k3s']['version']} is not the local cell's {image}")
    if CONFIG["k3s"]["version"].replace("+", "%2B") not in CONFIG["k3s"]["url"]:
        problems.append("the k3s download is not the pinned version")
    for name, key in (("CNPG_CHART_URL", "url"), ("CNPG_CHART_SHA256", "sha256")):
        value = re.search(rf'const {name}: &str = "([^"]+)"', XTASK_CELL).group(1)
        if CONFIG["cnpg_chart"][key] != value:
            problems.append(f"{name} differs")
    tag = re.search(r'const CNPG_OPERATOR_TAG: &str =\s*"([^"]+)"', XTASK_CELL).group(1)
    if CONFIG["cnpg_chart"]["operator_tag"] != tag:
        problems.append("the operator tag differs")
    for name, key in (
        ("PLUGIN_CHART_URL", "url"),
        ("PLUGIN_CHART_SHA256", "sha256"),
        ("PLUGIN_IMAGE_TAG", "image_tag"),
        ("PLUGIN_SIDECAR_TAG", "sidecar_tag"),
    ):
        value = re.search(rf'const {name}: &str =\s*"([^"]+)"', XTASK_CELL).group(1)
        if CONFIG["barman_plugin"][key] != value:
            problems.append(f"{name} differs")
    return problems


def test_no_public_inbound():
    assert no_public_inbound(FIREWALL) == []
    seeded = copy.deepcopy(FIREWALL)
    seeded["inbound"].append({"protocol": "tcp", "ports": "22", "sources": {"addresses": ["0.0.0.0/0"]}})
    assert no_public_inbound(seeded)


def test_outbound_by_port():
    assert outbound_by_port(FIREWALL) == []
    seeded = copy.deepcopy(FIREWALL)
    seeded["outbound"].append(
        {"why": "x", "protocol": "tcp", "ports": "all", "destinations": {"addresses": ["0.0.0.0/0"]}}
    )
    assert outbound_by_port(seeded)


def test_firewall_rules_render_every_rule():
    inbound, outbound = cell1.firewall_rules("inbound"), cell1.firewall_rules("outbound")
    assert inbound.split() == [
        "protocol:tcp,ports:1-65535,tag:mandate-cell1",
        "protocol:udp,ports:1-65535,tag:mandate-cell1",
        "protocol:icmp,tag:mandate-cell1",
    ]
    assert len(outbound.split()) == len(FIREWALL["outbound"])
    assert "protocol:tcp,ports:443,address:0.0.0.0/0,address:::/0" in outbound.split()


@pytest.mark.parametrize("role", ["server", "agent"])
def test_user_data(role):
    document = user_data(role)
    assert downloads_checked(document) == []
    assert nothing_starts_k3s(document) == []
    assert ssh_hardened(document) == []
    assert pods_kept_from_metadata(document) == []
    assert document["mounts"][0][:2] == [f"/dev/disk/by-id/scsi-0DO_Volume_{role}-data", "/var/lib/rancher"]


def test_user_data_seeded_bugs_are_caught():
    def seeded(change) -> dict:
        document = user_data("server")
        change(files(document))
        return document

    def script(f, old, new):
        entry = f["/usr/local/sbin/cell1-firstboot"]
        entry["content"] = entry["content"].replace(old, new)

    unchecked = seeded(lambda f: script(f, "| sha256sum --check --strict", "| cat"))
    assert downloads_checked(unchecked)
    chained = seeded(lambda f: script(f, "\necho '", " && echo '"))
    assert downloads_checked(chained)
    started = seeded(lambda f: script(f, "systemctl daemon-reload\n", "systemctl enable --now k3s\n"))
    assert nothing_starts_k3s(started)
    rooted = seeded(
        lambda f: f["/etc/ssh/sshd_config.d/10-cell1.conf"].update(content="PermitRootLogin yes\n")
    )
    assert ssh_hardened(rooted)
    unblocked = seeded(
        lambda f: f["/etc/systemd/system/k3s.service"].update(
            content=f["/etc/systemd/system/k3s.service"]["content"].replace(
                "Requires=cell1-metadata-block", "X"
            )
        )
    )
    assert pods_kept_from_metadata(unblocked)


def test_ops_token_held():
    assert ops_token_held(user_data("server"), user_data("agent")) == []
    open_file = user_data("server")
    files(open_file)["/etc/cell1/credentials/ops-tunnel-token"]["permissions"] = "0644"
    assert ops_token_held(open_file, user_data("agent"))
    leaky = user_data("agent")
    leaky["write_files"].append({"path": "/etc/cell1/credentials/x", "content": OPS_TOKEN})
    assert ops_token_held(user_data("server"), leaky)


def test_pins_match_xtask():
    assert pins_match_xtask() == []
    saved = CONFIG["k3s"]["version"]
    CONFIG["k3s"]["version"] = "v1.34.0+k3s1"
    try:
        assert pins_match_xtask()
    finally:
        CONFIG["k3s"]["version"] = saved
    saved = CONFIG["barman_plugin"]["sidecar_tag"]
    CONFIG["barman_plugin"]["sidecar_tag"] = "v0.15.0"
    try:
        assert pins_match_xtask() == ["PLUGIN_SIDECAR_TAG differs"]
    finally:
        CONFIG["barman_plugin"]["sidecar_tag"] = saved


SECRET_SETTING = re.compile(r"(token|secret|password|key)\"?\s*[:=]\s*\"?[A-Za-z0-9+/_-]{20,}", re.IGNORECASE)


def test_settings_hold_no_secret():
    """cell1.json, firewall.json, the docs and the hosted values are settings: no token or key."""
    pattern = SECRET_SETTING
    assert pattern.search('{"api_token": "' + SECRETS["cf"] + '"}'), "the check sees a planted token"
    assert pattern.search("secret_access_key = " + SECRETS["r2-key"])
    paths = [HOSTED / "cell1.json", HOSTED / "firewall.json", HOSTED / "RUNBOOK.md", HOSTED / "README.md"]
    for path in [*paths, *sorted((HOSTED / "values").iterdir())]:
        assert not pattern.search(path.read_text()), path


def recorders(tmp_path: Path, answers: dict[str, str] | None = None) -> Path:
    """A directory of stand-ins for every external tool: each appends its argv and stdin to a log
    and prints the canned answer for its command, if any."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    log = tmp_path / "calls.jsonl"
    for tool in TOOLS:
        script = bin_dir / tool
        script.write_text(
            f"""#!{sys.executable}
import json, os, stat, sys
data = "" if sys.stdin.isatty() else sys.stdin.read()
args = sys.argv[1:]
record = {{"argv": [{tool!r}, *args], "stdin": data, "env": dict(os.environ)}}
if "--user-data-file" in args:
    path = args[args.index("--user-data-file") + 1]
    record["user_data"] = open(path).read()
    record["user_data_mode"] = stat.S_IMODE(os.stat(path).st_mode)
with open({str(log)!r}, "a") as log:
    log.write(json.dumps(record) + "\\n")
answers = {json.dumps(answers or {})}
line = " ".join([{tool!r}, *sys.argv[1:]])
for needle, answer in answers.items():
    if needle in line:
        print(answer)
        break
"""
        )
        script.chmod(0o755)
    return log


def calls(log: Path) -> list[dict]:
    if not log.exists():
        return []
    return [json.loads(line) for line in log.read_text().splitlines()]


# The founder's archive, as the runbook's gate 5 has them pass it; documentation-range values only.
ARCHIVE_ARGS = [
    "--archive-destination",
    "s3://cell1-archive-example/ws1",
    "--archive-endpoint",
    "https://archive.example.com",
    "--archive-egress-cidr",
    "192.0.2.0/24",
]
HOSTED_ARGS = [
    *ARCHIVE_ARGS,
    "--zone",
    "owlhead.ai",
    "--cf-account-id",
    "acct",
    "--cf-zone-id",
    "zone",
    "--access-email",
    "founder@example.com",
    "--ghcr-user",
    "founder",
    "--r2-endpoint",
    "https://acct.r2.cloudflarestorage.com",
]


def secret_files(tmp_path: Path) -> list[str]:
    paths = {}
    for name, value in SECRETS.items():
        path = tmp_path / f"{name}.secret"
        path.write_text(value + "\n")
        path.chmod(0o600)
        paths[name] = str(path)
    return [
        "--cf-token-file",
        paths["cf"],
        "--ghcr-token-file",
        paths["ghcr"],
        "--r2-access-key-id-file",
        paths["r2-id"],
        "--r2-secret-access-key-file",
        paths["r2-key"],
        "--archive-access-key-id-file",
        paths["archive-id"],
        "--archive-secret-access-key-file",
        paths["archive-key"],
    ]


@pytest.fixture
def isolated(tmp_path, monkeypatch):
    monkeypatch.setattr(cell1, "STATE", tmp_path / "state")
    monkeypatch.setattr(cell1.time, "sleep", lambda _: None)
    log = recorders(tmp_path)
    monkeypatch.setenv("PATH", f"{tmp_path / 'bin'}{os.pathsep}{os.environ['PATH']}")
    key = tmp_path / "id.pub"
    key.write_text(KEY + "\n")
    return tmp_path, log, key


def no_network(*_args, **_kwargs):
    raise AssertionError("a dry run made a network request")


def test_dry_run_calls_nothing(isolated, monkeypatch):
    tmp_path, log, key = isolated
    monkeypatch.setattr(cell1.urllib.request, "urlopen", no_network)
    out = io.StringIO()
    argv = ["--dry-run", "all", *HOSTED_ARGS, "--ssh-public-key", str(key), "--published", "PUBLISHED"]
    assert cell1.main([*argv, *secret_files(tmp_path)], out) == 0
    assert calls(log) == [], "a dry run ran a tool"
    assert not (tmp_path / "state").exists(), "a dry run wrote state"
    text = out.getvalue()
    for value in SECRETS.values():
        assert value not in text
    for expected in (
        "+ doctl compute firewall create --name mandate-cell1 --tag-names mandate-cell1",
        "+ doctl compute droplet create cell1-a",
        "+ doctl compute droplet create cell1-b",
        "+ doctl compute volume create cell1-a-data",
        "+ doctl vpcs create --name mandate-cell1 --region nyc3",
        "+ cloudflare PUT /accounts/acct/cfd_tunnel/<tunnel id of cell1-ops>/configurations",
        "+ cloudflare POST /accounts/acct/access/apps",
        "+ cloudflared access tcp --hostname kube-cell1.owlhead.ai",
        "+ helm --kubeconfig",
        "deploy/hosted/values/cell1.yaml",
    ):
        assert expected in text, expected
    assert text.index("doctl compute firewall create") < text.index("doctl compute droplet create")
    assert text.index("droplet create cell1-a") < text.index("droplet create cell1-b")


def test_k3d_dry_run_calls_nothing(isolated):
    _, log, _ = isolated
    out = io.StringIO()
    assert (
        cell1.main(["--dry-run", "--target", "k3d", "all", "--local-image", "mandate.local/mandate:abc"], out)
        == 0
    )
    assert calls(log) == []
    text = out.getvalue()
    assert "+ k3d cluster create mandate-hosted" in text and "--agents 1" in text
    assert "ingress.cloudflared.enabled=false" in text and "localImages=true" in text
    assert "+ cloudflare" not in text and "+ doctl" not in text and "+ ssh" not in text


class FakeResponse(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


def fake_cloudflare(requests: list):
    def urlopen(request, timeout=0):
        path = request.full_url.removeprefix(cell1.API)
        requests.append((request.get_method(), path, request.headers, request.data))
        assert request.headers.get("Authorization") == f"Bearer {SECRETS['cf']}"
        if path.endswith("/token"):
            result = APP_TOKEN if "app-id" in path else OPS_TOKEN
        elif "/cfd_tunnel?name=" in path:
            result = [{"id": "app-id" if "cell1-app" in path else "ops-id"}]
        else:
            result = []
        return FakeResponse(json.dumps({"success": True, "result": result}).encode())

    return urlopen


def test_recorded_cluster_puts_every_secret_on_stdin_only(isolated, monkeypatch):
    """The real `cluster` step, against recorders and a fake Cloudflare API: each secret reaches its
    Secret manifest on kubectl's stdin, and never argv, the environment, or the output."""
    tmp_path, log, _ = isolated
    requests = []
    monkeypatch.setattr(cell1.urllib.request, "urlopen", fake_cloudflare(requests))
    monkeypatch.setattr(cell1, "fetch_chart", lambda plan, key, file: tmp_path / file)
    monkeypatch.setattr(
        cell1, "open_kube_tunnel", lambda argv: type("P", (), {"terminate": lambda s: None})()
    )
    published = tmp_path / "PUBLISHED"
    published.write_text(
        "ghcr.io/kunwarshivam/mandate/mandate@sha256:"
        + "a" * 64
        + "  mandate\n"
        + "ghcr.io/kunwarshivam/mandate/mandate-agentd@sha256:"
        + "b" * 64
        + "  mandate-agentd\n"
    )
    out = io.StringIO()
    argv = ["cluster", *HOSTED_ARGS, *secret_files(tmp_path), "--published", str(published)]
    assert cell1.main(argv, io.StringIO()) == 1, "cluster runs only with --accept-dev-tier"
    assert calls(log) == []
    argv.append("--accept-dev-tier")
    assert cell1.main(argv, out) == 0
    recorded = calls(log)
    planted = [*SECRETS.values(), APP_TOKEN]
    for call in recorded:
        for value in planted:
            assert value not in " ".join(call["argv"]), call["argv"]
            assert value not in json.dumps(call["env"]), "a secret in a tool's environment"
    for value in planted:
        assert value not in out.getvalue()
    manifests = [
        json.loads(c["stdin"]) for c in recorded if "apply" in c["argv"] and c["argv"][-2:] == ["-f", "-"]
    ]
    assert all("--server-side" in c["argv"] for c in recorded if "apply" in c["argv"]), "no last-applied copy"
    secrets_made = {m["metadata"]["name"]: m for m in manifests if m["kind"] == "Secret"}
    assert set(secrets_made) == {"ghcr-pull", "object-store-r2", "cloudflared-tunnel", "journal-archive-key"}

    def decoded(name, key):
        return base64.b64decode(secrets_made[name]["data"][key]).decode()

    auth = json.loads(decoded("ghcr-pull", ".dockerconfigjson"))["auths"]["ghcr.io"]["auth"]
    assert base64.b64decode(auth).decode() == f"founder:{SECRETS['ghcr']}"
    assert secrets_made["ghcr-pull"]["type"] == "kubernetes.io/dockerconfigjson"
    assert decoded("cloudflared-tunnel", "token") == APP_TOKEN
    assert decoded("object-store-r2", "secret-access-key") == SECRETS["r2-key"]
    assert decoded("journal-archive-key", "ACCESS_KEY_ID") == SECRETS["archive-id"]
    assert decoded("journal-archive-key", "ACCESS_SECRET_KEY") == SECRETS["archive-key"]
    journal = next(c["argv"] for c in recorded if c["argv"][:1] == ["helm"] and "journal" in c["argv"])
    assert "archive.destinationPath=s3://cell1-archive-example/ws1" in journal
    assert 'archive.egress={"cidrs":["192.0.2.0/24"],"port":443,"namespace":"","podSelector":{}}' in journal
    helm_calls = [c["argv"] for c in recorded if c["argv"][:1] == ["helm"]]
    assert not [a for a in helm_calls if "archive-store" in a], "cell #1 installed the local test store"
    releases = [a[a.index("--install") + 1] for a in helm_calls if "--install" in a]
    assert releases.index("barman-cloud") < releases.index("journal"), releases
    workspace = next(c["argv"] for c in recorded if c["argv"][:1] == ["helm"] and "workspace" in c["argv"])
    assert "images.mandate=ghcr.io/kunwarshivam/mandate/mandate@sha256:" + "a" * 64 in workspace
    assert "localImages=true" not in workspace
    patch = next(c["argv"] for c in recorded if "patch" in c["argv"])
    assert json.loads(patch[-1]) == {"imagePullSecrets": [{"name": "ghcr-pull"}]}


def test_a_secret_on_a_command_line_is_refused():
    plan = cell1.Plan(dry_run=False, out=io.StringIO())
    plan.secret(SECRETS["ghcr"])
    with pytest.raises(cell1.Refused):
        plan.run(["kubectl", "create", "secret", f"--docker-password={SECRETS['ghcr']}"])


def test_a_secret_file_others_can_read_is_refused(tmp_path):
    plan = cell1.Plan(dry_run=False, out=io.StringIO())
    path = tmp_path / "token"
    path.write_text("x" * 20)
    path.chmod(0o640)
    with pytest.raises(cell1.Refused, match="readable by others"):
        cell1.secret_file(plan, str(path), "a token")
    path.chmod(0o600)
    assert cell1.secret_file(plan, str(path), "a token") == "x" * 20
    assert stat.S_IMODE(path.stat().st_mode) == 0o600


def test_recorded_nodes_pass_the_join_token_on_stdin(isolated, monkeypatch):
    tmp_path, log, _ = isolated
    log.unlink(missing_ok=True)
    answers = {
        "droplet list": "cell1-a 10.110.0.2\ncell1-b 10.110.0.3",
        "agent-token": JOIN_TOKEN,
        "k3s.yaml": "server: https://127.0.0.1:6443",
    }
    (tmp_path / "bin").rename(tmp_path / "old-bin")
    log = recorders(tmp_path, answers)
    out = io.StringIO()
    assert (
        cell1.main(["nodes", "--zone", "owlhead.ai", "--ssh-public-key", str(tmp_path / "id.pub")], out) == 0
    )
    recorded = calls(log)
    join = [c for c in recorded if "cell1-token agent-token" in " ".join(c["argv"])]
    assert len(join) == 1 and join[0]["stdin"] == JOIN_TOKEN and "cell1-b" in join[0]["argv"]
    assert all(JOIN_TOKEN not in " ".join(c["argv"]) for c in recorded)
    assert JOIN_TOKEN not in out.getvalue()
    kubeconfig = tmp_path / "state" / "kubeconfig"
    assert kubeconfig.read_text() == f"server: https://127.0.0.1:{CONFIG['kube_local_port']}"
    assert stat.S_IMODE(kubeconfig.stat().st_mode) == 0o600
    config = (tmp_path / "state" / "ssh_config").read_text()
    assert "ProxyCommand cloudflared access ssh --hostname %h" in config
    assert "HostName 10.110.0.3" in config and "ProxyJump cell1-a" in config
    assert config.count(f"IdentityFile {tmp_path / 'id'}\n") == 2


def no_role_placeholder(document: dict) -> list[str]:
    """Every node file is rendered for its role: no `__K3S_ROLE__` is left, and k3s runs that role."""
    text = json.dumps(document)
    problems = ["a role placeholder is left"] if "__K3S_ROLE__" in text else []
    return problems


@pytest.mark.parametrize("role", ["server", "agent"])
def test_roles_rendered(role):
    document = user_data(role)
    assert no_role_placeholder(document) == []
    assert (
        f"ExecStart=/usr/local/bin/k3s {role}\n"
        in files(document)["/etc/systemd/system/k3s.service"]["content"]
    )
    unit = files(document)["/etc/systemd/system/k3s.service"]["content"]
    assert "RequiresMountsFor=/var/lib/rancher" in unit, "k3s never starts on the root disk"
    seeded = copy.deepcopy(document)
    files(seeded)["/etc/systemd/system/k3s.service"]["content"] = "ExecStart=/usr/local/bin/k3s __K3S_ROLE__"
    assert no_role_placeholder(seeded)


def test_the_ops_tunnel_starts_before_k3s():
    """A first-boot failure after the tunnel still leaves the founder a way in to read it."""
    script = files(user_data("server"))["/usr/local/sbin/cell1-firstboot"]["content"]
    assert script.index("enable --now cloudflared-ops") < script.index("k3s/releases/download")


def cloudflare_api(requests: list):
    """A Cloudflare API that holds nothing yet: every listing is empty, every POST makes an id."""

    def urlopen(request, timeout=0):
        path = request.full_url.removeprefix(cell1.API)
        body = json.loads(request.data) if request.data else None
        requests.append((request.get_method(), path, body))
        assert request.headers.get("Authorization") == f"Bearer {SECRETS['cf']}"
        if path.endswith("/token"):
            result = OPS_TOKEN
        elif request.get_method() == "POST":
            result = {"id": f"id-{len(requests)}"}
        elif request.get_method() == "GET":
            result = []
        else:
            result = {}
        return FakeResponse(json.dumps({"success": True, "result": result}).encode())

    return urlopen


DOCTL_EMPTY = {
    "vpcs create": "vpc-1",
    "volume create cell1-a-data": "vol-a",
    "volume create cell1-b-data": "vol-b",
    "droplet create cell1-a": "10.110.0.2",
    "droplet create cell1-b": "10.110.0.3",
}


def fresh_recorders(tmp_path: Path, answers: dict[str, str]) -> Path:
    bin_dir = tmp_path / "bin"
    if bin_dir.exists():
        for f in bin_dir.iterdir():
            f.unlink()
        bin_dir.rmdir()
    (tmp_path / "calls.jsonl").unlink(missing_ok=True)
    return recorders(tmp_path, answers)


def test_recorded_cloudflare_makes_access_before_each_name(isolated, monkeypatch):
    tmp_path, _, _ = isolated
    requests = []
    monkeypatch.setattr(cell1.urllib.request, "urlopen", cloudflare_api(requests))
    out = io.StringIO()
    assert cell1.main(["cloudflare", *HOSTED_ARGS, *secret_files(tmp_path)], out) == 0
    tunnels = [b for m, p, b in requests if m == "POST" and p.endswith("/cfd_tunnel")]
    assert [t["name"] for t in tunnels] == ["cell1-ops", "cell1-app"]
    assert all(
        t["config_src"] == "cloudflare" and len(base64.b64decode(t["tunnel_secret"])) == 32 for t in tunnels
    )
    for t in tunnels:
        assert t["tunnel_secret"] not in out.getvalue(), "a tunnel secret was printed"
    configs = [b for m, p, b in requests if m == "PUT" and p.endswith("/configurations")]
    ops = configs[0]["config"]["ingress"]
    assert ops == [
        {"hostname": "ssh-cell1.owlhead.ai", "service": "ssh://localhost:22"},
        {"hostname": "kube-cell1.owlhead.ai", "service": "tcp://localhost:6443"},
        {"service": "http_status:404"},
    ]
    assert configs[1]["config"]["ingress"][0] == {
        "hostname": "api.owlhead.ai",
        "service": "http://api.ws-ws1.svc.cluster.local:8080",
    }
    policy = next(b for m, p, b in requests if m == "POST" and p.endswith("/access/policies"))
    assert policy["decision"] == "allow" and policy["include"] == [
        {"email": {"email": "founder@example.com"}}
    ]
    made = [
        ("app" if p.endswith("/access/apps") else "dns", b["name"])
        for m, p, b in requests
        if m == "POST" and (p.endswith("/access/apps") or p.endswith("/dns_records"))
    ]
    for host in ("ssh-cell1.owlhead.ai", "kube-cell1.owlhead.ai", "api.owlhead.ai"):
        assert made.index(("app", host)) < made.index(("dns", host)), f"{host} resolves before Access"
    apps = [b for m, p, b in requests if m == "POST" and p.endswith("/access/apps")]
    assert len(apps) == 3
    assert all(a["policies"] == [{"id": policy_id(requests), "precedence": 1}] for a in apps)
    cnames = [b for m, p, b in requests if m == "POST" and p.endswith("/dns_records")]
    assert all(c["proxied"] and c["content"].endswith(".cfargotunnel.com") for c in cnames)


def policy_id(requests) -> str:
    index = next(i for i, (m, p, _) in enumerate(requests) if m == "POST" and p.endswith("/access/policies"))
    return f"id-{index + 1}"


def test_recorded_droplets_firewall_first_and_the_token_only_in_user_data(isolated, monkeypatch):
    tmp_path, _, key = isolated
    monkeypatch.setattr(cell1.urllib.request, "urlopen", cloudflare_api([]))
    log = fresh_recorders(tmp_path, DOCTL_EMPTY)
    out = io.StringIO()
    argv = ["droplets", *HOSTED_ARGS, *secret_files(tmp_path), "--ssh-public-key", str(key)]
    assert cell1.main(argv, out) == 0
    recorded = calls(log)
    lines = [" ".join(c["argv"]) for c in recorded]
    first_create = next(i for i, line in enumerate(lines) if "droplet create" in line)
    assert any("firewall create" in line for line in lines[:first_create]), "a droplet before its firewall"
    creates = [c for c in recorded if "droplet create" in " ".join(c["argv"])]
    assert [c["argv"][4] for c in creates] == ["cell1-a", "cell1-b"]
    server, agent = (json.loads(c["user_data"].split("\n", 1)[1]) for c in creates)
    assert all(c["user_data_mode"] == 0o600 for c in creates)
    assert ops_token_held(server, agent) == []
    assert files(server)["/etc/cell1/credentials/ops-tunnel-token"]["content"] == OPS_TOKEN
    assert "cell1-k3s-config agent 10.110.0.2" in files(agent)["/usr/local/sbin/cell1-firstboot"]["content"]
    for call in recorded:
        assert OPS_TOKEN not in " ".join(call["argv"]) and OPS_TOKEN not in json.dumps(call["env"])
    assert OPS_TOKEN not in out.getvalue()
    assert not list((tmp_path / "state").glob("user-data-*")), "user data left on disk"


def test_recorded_droplets_twice_creates_nothing_again(isolated, monkeypatch):
    tmp_path, _, key = isolated
    monkeypatch.setattr(cell1.urllib.request, "urlopen", cloudflare_api([]))
    fingerprint = cell1.md5_fingerprint(KEY)
    log = fresh_recorders(
        tmp_path,
        {
            "tag list": "mandate-cell1",
            "firewall list": "fw-1 mandate-cell1",
            "vpcs list": "vpc-1 mandate-cell1",
            "ssh-key list": fingerprint,
            "volume list": "vol-a cell1-a-data\nvol-b cell1-b-data",
            "droplet list": "cell1-a 10.110.0.2\ncell1-b",
        },
    )
    argv = ["droplets", *HOSTED_ARGS, *secret_files(tmp_path), "--ssh-public-key", str(key)]
    assert cell1.main(argv, io.StringIO()) == 0
    lines = [" ".join(c["argv"]) for c in calls(log)]
    assert not [line for line in lines if " create" in line or " import " in line], lines
    assert any(line.startswith("doctl compute firewall update fw-1 ") for line in lines)


def without_archive(argv: list[str]) -> list[str]:
    flags = {"--archive-destination", "--archive-endpoint", "--archive-egress-cidr"}
    out, skip = [], False
    for arg in argv:
        if skip:
            skip = False
        elif arg in flags:
            skip = True
        else:
            out.append(arg)
    return out


@pytest.mark.parametrize("dry_run", [True, False])
def test_hosted_cluster_refuses_without_an_archive(isolated, monkeypatch, capsys, dry_run):
    """Cell #1's archive bucket is the founder's to choose and pay for (DEC-79, DEC-995): with
    none named, `cluster` refuses before it calls a tool or the Cloudflare API, dry run included,
    and names the gate (DEC-2006)."""
    tmp_path, log, _ = isolated
    monkeypatch.setattr(cell1.urllib.request, "urlopen", no_network)
    argv = ["cluster", *HOSTED_ARGS, *secret_files(tmp_path), "--published", "PUBLISHED", "--accept-dev-tier"]
    argv = without_archive(argv)
    assert "--archive-destination" not in argv
    if dry_run:
        argv.insert(0, "--dry-run")
    assert cell1.main(argv, io.StringIO()) == 1
    assert calls(log) == []
    err = capsys.readouterr().err
    assert "has no destination" in err and "DEC-2006" in err and "--archive-destination" in err
    for flag, bad in (
        ("--archive-destination", "s3://Bad_Bucket"),
        ("--archive-endpoint", "http://archive.example.com"),
        ("--archive-egress-cidr", "0.0.0.0/0"),
        ("--archive-egress-cidr", "999.1.1.1/24"),
    ):
        given = dict(zip(ARCHIVE_ARGS[::2], ARCHIVE_ARGS[1::2]), **{flag: bad})
        seeded = [*argv, *(arg for pair in given.items() for arg in pair)]
        assert cell1.main(seeded, io.StringIO()) == 1, flag
        assert flag in capsys.readouterr().err
    assert calls(log) == []


def real_all(tmp_path: Path, key: Path) -> list[str]:
    return ["all", *HOSTED_ARGS, *secret_files(tmp_path), "--ssh-public-key", str(key), "--published", "PUBLISHED"]


def drop_flag(argv: list[str], flag: str) -> list[str]:
    at = argv.index(flag)
    return argv[:at] + argv[at + 2 :]


def refused_before_any_step(argv: list[str], log: Path, capsys) -> str:
    """A real hosted `all` that refuses up front: no doctl, cloudflared, ssh, kubectl or helm call,
    and no Cloudflare request (urlopen is blocked), so nothing is bought."""
    assert cell1.main(argv, io.StringIO()) == 1
    assert calls(log) == [], "a tool ran before the pre-flight refused"
    return capsys.readouterr().err


def test_real_all_without_an_archive_refuses_before_cloudflare_and_droplets(isolated, monkeypatch, capsys):
    """`all` would spend money in `cloudflare` and `droplets` before `cluster` refused for want of
    the founder's archive (DEC-2006); the pre-flight refuses first, with `cluster`'s message."""
    tmp_path, log, key = isolated
    monkeypatch.setattr(cell1.urllib.request, "urlopen", no_network)
    argv = without_archive([*real_all(tmp_path, key), "--accept-dev-tier"])
    err = refused_before_any_step(argv, log, capsys)
    assert "has no destination" in err and "DEC-2006" in err and "--archive-destination" in err
    for flag, bad in (
        ("--archive-destination", "s3://Bad_Bucket"),
        ("--archive-endpoint", "http://archive.example.com"),
        ("--archive-egress-cidr", "999.1.1.1/24"),
    ):
        given = dict(zip(ARCHIVE_ARGS[::2], ARCHIVE_ARGS[1::2]), **{flag: bad})
        seeded = [*argv, *(arg for pair in given.items() for arg in pair)]
        assert flag in refused_before_any_step(seeded, log, capsys), flag


def test_real_all_without_the_dev_tier_answer_refuses_up_front(isolated, monkeypatch, capsys):
    tmp_path, log, key = isolated
    monkeypatch.setattr(cell1.urllib.request, "urlopen", no_network)
    err = refused_before_any_step(real_all(tmp_path, key), log, capsys)
    assert "--accept-dev-tier is required" in err


SECRET_FLAGS = {
    "--ghcr-token-file": "ghcr",
    "--cf-token-file": "cf",
    "--r2-access-key-id-file": "r2-id",
    "--r2-secret-access-key-file": "r2-key",
    "--archive-access-key-id-file": "archive-id",
    "--archive-secret-access-key-file": "archive-key",
}


def test_the_pre_flight_checks_every_secret_file_cluster_reads():
    """The pre-flight's list is exactly the secret files `cluster` reads on cell #1."""
    flags = {"--" + attr.replace("_", "-") for attr, _ in cell1.CLUSTER_SECRET_FILES}
    assert flags == set(SECRET_FLAGS)


@pytest.mark.parametrize("flag", sorted(SECRET_FLAGS))
@pytest.mark.parametrize("fault", ["missing", "readable"])
def test_real_all_refuses_up_front_without_a_private_secret_file(isolated, monkeypatch, capsys, flag, fault):
    tmp_path, log, key = isolated
    monkeypatch.setattr(cell1.urllib.request, "urlopen", no_network)
    argv = [*real_all(tmp_path, key), "--accept-dev-tier"]
    if fault == "missing":
        argv = drop_flag(argv, flag)
        expected = "no file given"
    else:
        (tmp_path / f"{SECRET_FLAGS[flag]}.secret").chmod(0o644)
        expected = "chmod 600"
    assert expected in refused_before_any_step(argv, log, capsys)


def test_the_pre_flight_leaves_dry_runs_and_k3d_alone(isolated, monkeypatch):
    """A dry run still prints every step up to `cluster`'s refusal, and k3d needs none of it."""
    tmp_path, log, key = isolated
    monkeypatch.setattr(cell1.urllib.request, "urlopen", no_network)
    out = io.StringIO()
    assert cell1.main(["--dry-run", *without_archive(real_all(tmp_path, key))], out) == 1
    assert "+ doctl compute droplet create cell1-a" in out.getvalue()
    assert calls(log) == []


@pytest.mark.parametrize(
    "cidr, ok",
    [
        ("192.0.2.0/24", True),
        ("192.0.2.7/32", True),
        ("999.1.1.1/24", False),
        ("192.0.2.1/24", False),
        ("0.0.0.0/0", False),
        ("192.0.0.0/16", False),
        ("192.0.2.0", False),
    ],
)
def test_archive_cidr(cidr, ok):
    assert cell1.archive_cidr_ok(cidr) is ok


def test_k3d_archive_is_the_local_cells():
    """The k3d trial archives exactly where the local cell does: the archive block of
    values/k3d-journal-archive.yaml is deploy/cell/values/k3d-journal-db.yaml's."""

    def block(path: Path) -> list[str]:
        lines = path.read_text().splitlines()
        return lines[lines.index("archive:") :]

    assert block(HOSTED / "values" / "k3d-journal-archive.yaml") == block(LOCAL_JOURNAL_VALUES)
    assert "destinationPath" not in (HOSTED / "values" / "cell1-journal-db.yaml").read_text()


def recorded_journal_install(tmp_path: Path, monkeypatch, argv: list[str]) -> list[str]:
    """The `helm upgrade --install journal` argv `cluster` runs, recorded."""
    log = tmp_path / "calls.jsonl"
    monkeypatch.setattr(cell1, "fetch_chart", lambda plan, key, file: tmp_path / file)
    monkeypatch.setattr(
        cell1, "open_kube_tunnel", lambda argv: type("P", (), {"terminate": lambda s: None})()
    )
    assert cell1.main(argv, io.StringIO()) == 0
    return next(
        c["argv"] for c in calls(log) if c["argv"][:1] == ["helm"] and "journal" in c["argv"][:6]
    )


def as_template(install: list[str]) -> list[str]:
    """The recorded install as `helm template`, with the Kubernetes API named, since no cluster is
    there to look it up, and without the flags only an install takes."""
    args = install[1:]
    if args[:1] == ["--kubeconfig"]:
        args = args[2:]
    assert args[:3] == ["upgrade", "--install", "journal"], args
    rest, out, skip = args[3:], [], False
    for arg in rest:
        if skip:
            skip = False
        elif arg == "--timeout":
            skip = True
        elif arg != "--wait":
            out.append(arg)
    return [
        HELM,
        "template",
        "journal",
        *out,
        "--set-json",
        'kubeApi={"cidrs":["10.43.0.1/32"],"ports":[443]}',
    ]


def helm_or_skip() -> None:
    if HELM is None:
        if os.environ.get("MANDATE_CELL_HELM_REQUIRED"):
            raise RuntimeError("MANDATE_CELL_HELM_REQUIRED is set but helm is not installed")
        pytest.skip("helm is not installed (deploy/cell/README.md)")


def rendered(argv: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(argv, capture_output=True, text=True, check=False)


def test_k3d_journal_values_meet_the_chart(isolated, monkeypatch):
    """What `cell1.py --target k3d cluster` hands helm for journal-db passes the chart's schema and
    renders a synchronous standby archiving to the test store. Nothing rendered this file before,
    so the chart's E21-41 requirements reached it only in CI's hosted-k3d job."""
    helm_or_skip()
    tmp_path, _, _ = isolated
    install = recorded_journal_install(tmp_path, monkeypatch, ["--target", "k3d", "cluster"])
    done = rendered(as_template(install))
    assert done.returncode == 0, done.stderr
    docs = [json.loads(line) for line in done.stdout.splitlines() if line.strip().startswith("{")]
    cluster = next(d for d in docs if d["kind"] == "Cluster")
    store = next(d for d in docs if d["kind"] == "ObjectStore")
    assert cluster["spec"]["instances"] == 2
    assert cluster["spec"]["affinity"]["podAntiAffinityType"] == "required"
    assert store["spec"]["configuration"]["destinationPath"] == "s3://journal-archive/ws1"
    assert store["spec"]["configuration"]["endpointURL"] == "http://archive-store.archive-store.svc:9000"
    seeded = as_template(install)
    at = seeded.index(str(HOSTED / "values" / "k3d-journal-archive.yaml"))
    del seeded[at - 1 : at + 1]
    failed = rendered(seeded)
    assert failed.returncode != 0 and "/archive/destinationPath" in failed.stderr, failed.stderr


def test_hosted_journal_values_meet_the_chart(isolated, monkeypatch):
    """Cell #1's journal-db, with the founder's archive given, passes the chart's schema and
    archives only to the named CIDRs over HTTPS."""
    helm_or_skip()
    tmp_path, _, _ = isolated
    requests = []
    monkeypatch.setattr(cell1.urllib.request, "urlopen", fake_cloudflare(requests))
    published = tmp_path / "PUBLISHED"
    published.write_text("ghcr.io/kunwarshivam/mandate/mandate@sha256:" + "a" * 64 + "  mandate\n")
    argv = ["cluster", *HOSTED_ARGS, *secret_files(tmp_path), "--published", str(published), "--accept-dev-tier"]
    done = rendered(as_template(recorded_journal_install(tmp_path, monkeypatch, argv)))
    assert done.returncode == 0, done.stderr
    docs = [json.loads(line) for line in done.stdout.splitlines() if line.strip().startswith("{")]
    assert next(d for d in docs if d["kind"] == "Cluster")["spec"]["instances"] == 2
    store = next(d for d in docs if d["kind"] == "ObjectStore")["spec"]["configuration"]
    assert store["destinationPath"] == "s3://cell1-archive-example/ws1"
    assert store["endpointURL"] == "https://archive.example.com"
    assert "192.0.2.0/24" in done.stdout
