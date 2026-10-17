"""Invariants of cell #1's hosted bring-up (E21-51, DEC-2000), on the committed files and on what
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
import stat
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
KEY = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl founder@laptop"
TOOLS = ("doctl", "cloudflared", "ssh", "kubectl", "helm", "curl", "k3d")
# Values planted in the secret files of the recorded run; none may reach argv or output.
SECRETS = {
    "cf": "cf-api-token-0123456789abcdef",
    "ghcr": "ghp_readpackages0123456789abcdef",
    "r2-id": "r2-access-key-id-0123456789",
    "r2-key": "r2-secret-access-key-0123456789abcdef",
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


def test_settings_hold_no_secret():
    """cell1.json, firewall.json and the hosted values are settings: no token, key or account id."""
    pattern = re.compile(r"(token|secret|password|key)\"?\s*[:=]\s*\"?[A-Za-z0-9+/_-]{20,}", re.IGNORECASE)
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
import json, sys
data = "" if sys.stdin.isatty() else sys.stdin.read()
with open({str(log)!r}, "a") as log:
    log.write(json.dumps({{"argv": [{tool!r}, *sys.argv[1:]], "stdin": data}}) + "\\n")
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


HOSTED_ARGS = [
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
    monkeypatch.setattr(cell1, "fetch_cnpg_chart", lambda plan: tmp_path / "cnpg.tgz")
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
    assert cell1.main(argv, out) == 0
    recorded = calls(log)
    planted = [*SECRETS.values(), APP_TOKEN]
    for call in recorded:
        for value in planted:
            assert value not in " ".join(call["argv"]), call["argv"]
    for value in planted:
        assert value not in out.getvalue()
    manifests = [json.loads(c["stdin"]) for c in recorded if c["argv"][-3:] == ["apply", "-f", "-"]]
    secrets_made = {m["metadata"]["name"]: m for m in manifests if m["kind"] == "Secret"}
    assert set(secrets_made) == {"ghcr-pull", "object-store-r2", "cloudflared-tunnel"}

    def decoded(name, key):
        return base64.b64decode(secrets_made[name]["data"][key]).decode()

    auth = json.loads(decoded("ghcr-pull", ".dockerconfigjson"))["auths"]["ghcr.io"]["auth"]
    assert base64.b64decode(auth).decode() == f"founder:{SECRETS['ghcr']}"
    assert secrets_made["ghcr-pull"]["type"] == "kubernetes.io/dockerconfigjson"
    assert decoded("cloudflared-tunnel", "token") == APP_TOKEN
    assert decoded("object-store-r2", "secret-access-key") == SECRETS["r2-key"]
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
