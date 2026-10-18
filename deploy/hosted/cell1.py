#!/usr/bin/env python3
"""Stand up cell #1 on two DigitalOcean droplets behind Cloudflare Tunnel and Access (E21-51, DEC-2001).

The founder runs this from their own machine with their own tokens (deploy/hosted/RUNBOOK.md); agents
never do. Every step prints each command and each Cloudflare API request before it runs, and
`--dry-run` prints them all and calls nothing: no doctl, cloudflared, ssh, kubectl, helm or API call.

    cell1.py [--dry-run] cloudflare   tunnels, their routes, DNS, the Access policy and applications
    cell1.py [--dry-run] droplets     tag, firewall, VPC, SSH key, volumes and droplets (spends money)
    cell1.py [--dry-run] nodes        waits for cloud-init, starts k3s, joins the agent, fetches kubeconfig
    cell1.py [--dry-run] cluster      secrets, the CloudNativePG operator and its backup plugin,
                                      journal-db, the workspace chart; refused until the founder
                                      names the journal's archive (--archive-*, DEC-2006)
    cell1.py [--dry-run] check        the pods, and the API hostname answering only through Access
    cell1.py [--dry-run] all          every step above, in order

    cell1.py --target k3d [--dry-run] nodes|cluster|all
        The same `cluster` step against a local two-node k3d cluster, the proof before anything is
        bought: no Cloudflare and no DigitalOcean, the tunnel off, the change's own image if named,
        and the journal archiving to the local cell's test store, as `cargo xtask cell up` does.

Secrets are only ever file paths (each must be a regular file, mode 0600 or narrower): the values
are read into memory and reach their destination on a process's stdin or in a request header, never
on a command line, in the environment, in a log, or in this program's output. doctl uses the
founder's own `doctl auth init` context, which this program never reads.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import ipaddress
import json
import os
import re
import secrets
import shlex
import stat
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
CELL = REPO / "deploy" / "cell"
CONFIG = json.loads((HERE / "cell1.json").read_text())
FIREWALL = json.loads((HERE / "firewall.json").read_text())
NODE = HERE / "node"
API = "https://api.cloudflare.com/client/v4"
SSH_UNREACHABLE = 255
STATE = Path(os.environ.get("MANDATE_CELL1_STATE", Path.home() / ".mandate-cell1"))


class Refused(Exception):
    """A precondition failed, or a command did; nothing after it ran."""

    def __init__(self, message: str, code: int | None = None):
        super().__init__(message)
        self.code = code


def placeholder(what: str) -> str:
    return f"<{what}>"


class Plan:
    """Runs commands and API requests, or in a dry run prints them and returns placeholders."""

    def __init__(self, dry_run: bool, out=sys.stdout):
        self.dry_run = dry_run
        self.out = out
        self.secret_values: list[str] = []

    def say(self, text: str) -> None:
        print(f"\n==> {text}", file=self.out, flush=True)

    def secret(self, value: str) -> str:
        if len(value) >= 8:
            self.secret_values.append(value)
        return value

    def guard(self, text: str) -> None:
        for value in self.secret_values:
            if value in text:
                raise Refused("a secret value reached a command line or a printed line; nothing was run")

    def show(self, line: str) -> None:
        self.guard(line)
        print(f"+ {line}", file=self.out, flush=True)

    def run(
        self,
        argv: list[str],
        *,
        stdin: bytes | None = None,
        stdin_note: str = "",
        capture: bool = False,
        result: str = "",
        check: bool = True,
        env: dict[str, str] | None = None,
    ) -> str:
        """Runs argv. stdin carries any secret; stdin_note says, without the value, what it is."""
        line = shlex.join(argv) + (f" < {stdin_note}" if stdin_note else "")
        self.show(line)
        if self.dry_run:
            return placeholder(result or "output")
        done = subprocess.run(
            argv,
            input=stdin,
            capture_output=capture,
            check=False,
            env={**os.environ, **(env or {})},
        )
        if check and done.returncode != 0:
            detail = done.stderr.decode(errors="replace").strip() if capture else ""
            message = f"`{argv[0]}` exited {done.returncode}" + (f": {detail}" if detail else "")
            raise Refused(message, done.returncode)
        return done.stdout.decode().strip() if capture else ""

    def lines(self, argv: list[str], result: str) -> list[list[str]]:
        """A doctl listing (`--format ... --no-header`), as rows of fields."""
        text = self.run(argv, capture=True, result=result)
        if self.dry_run:
            return []
        return [row.split() for row in text.splitlines() if row.strip()]


def secret_file(plan: Plan, path: str | None, what: str) -> str:
    """Reads one secret from a file the founder made, refusing one any other user can read."""
    if not path:
        raise Refused(f"{what}: no file given")
    if plan.dry_run:
        return placeholder(f"{what} from {path}")
    p = Path(path).expanduser()
    info = p.lstat()
    if not stat.S_ISREG(info.st_mode):
        raise Refused(f"{what}: {path} is not a regular file")
    if info.st_mode & 0o077:
        raise Refused(f"{what}: {path} is readable by others; chmod 600 it")
    value = p.read_text().strip()
    if not value or "\n" in value:
        raise Refused(f"{what}: {path} must hold exactly one line")
    return plan.secret(value)


class Cloudflare:
    """The Cloudflare API, with the founder's token in a header only."""

    def __init__(self, plan: Plan, token: str, account: str, zone_id: str):
        self.plan = plan
        self.token = token
        self.account = account
        self.zone_id = zone_id

    def call(self, method: str, path: str, body: dict | None = None, *, note: str = "", result: str = ""):
        shown = "" if body is None else " " + json.dumps(body)
        self.plan.show(f"cloudflare {method} {path}{shown}{f'  ({note})' if note else ''}")
        if self.plan.dry_run:
            return None
        return self.send(method, path, body)

    def send(self, method: str, path: str, body: dict | None = None):
        """One request, already shown; Cloudflare's own error messages, never the body, on failure."""
        request = urllib.request.Request(
            API + path,
            method=method,
            data=None if body is None else json.dumps(body).encode(),
            headers={"Authorization": f"Bearer {self.token}", "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                answer = json.load(response)
        except urllib.error.HTTPError as error:
            try:
                answer = json.loads(error.read() or b"{}")
            except ValueError:
                answer = {}
        except (urllib.error.URLError, TimeoutError) as error:
            raise Refused(f"Cloudflare {method} {path}: {error}") from None
        if not answer.get("success"):
            errors = [e.get("message", "") for e in answer.get("errors", [])]
            raise Refused(f"Cloudflare {method} {path} failed: {errors}")
        return answer["result"]

    def acct(self, rest: str) -> str:
        return f"/accounts/{self.account}{rest}"

    def upsert(self, listing: str, match, create: str, body: dict, what: str) -> str:
        """GET the listing; PUT the one item `match` picks, or POST a new one. Returns its id."""
        found = self.call("GET", listing)
        if self.plan.dry_run:
            self.call("PUT", f"{create}/<id>", body, note=f"if the GET found {what}")
            self.call("POST", create, body, note="otherwise")
            return placeholder(f"id of {what}")
        item = next((i for i in found if match(i)), None)
        if item is not None:
            self.call("PUT", f"{create}/{item['id']}", body)
            return item["id"]
        return self.call("POST", create, body)["id"]

    def tunnel(self, name: str) -> str:
        """The remotely configured tunnel `name`, made if absent. Cloudflare holds its secret,
        which this program makes and never prints."""
        found = self.call("GET", self.acct(f"/cfd_tunnel?name={name}&is_deleted=false"))
        if self.plan.dry_run:
            body = {"name": name, "config_src": "cloudflare", "tunnel_secret": "<32 random bytes, base64>"}
            self.call("POST", self.acct("/cfd_tunnel"), body, note="only if the GET found none")
            return placeholder(f"tunnel id of {name}")
        if found:
            return found[0]["id"]
        secret = self.plan.secret(base64.b64encode(secrets.token_bytes(32)).decode())
        body = {"name": name, "config_src": "cloudflare", "tunnel_secret": secret}
        self.plan.show(f"cloudflare POST {self.acct('/cfd_tunnel')} {{name: {name}, config_src: cloudflare}}")
        return self.send("POST", self.acct("/cfd_tunnel"), body)["id"]

    def routes(self, tunnel_id: str, routes: dict[str, str], zone: str) -> None:
        ingress = [{"hostname": f"{host}.{zone}", "service": service} for host, service in routes.items()]
        ingress.append({"service": "http_status:404"})
        self.call(
            "PUT", self.acct(f"/cfd_tunnel/{tunnel_id}/configurations"), {"config": {"ingress": ingress}}
        )

    def dns(self, fqdn: str, tunnel_id: str) -> None:
        record = {"type": "CNAME", "name": fqdn, "content": f"{tunnel_id}.cfargotunnel.com", "proxied": True}
        self.upsert(
            f"/zones/{self.zone_id}/dns_records?type=CNAME&name={fqdn}",
            lambda r: r["name"] == fqdn,
            f"/zones/{self.zone_id}/dns_records",
            record,
            f"the CNAME {fqdn}",
        )

    def policy(self, name: str, emails: list[str]) -> str:
        body = {
            "name": name,
            "decision": "allow",
            "include": [{"email": {"email": e}} for e in emails],
            "session_duration": CONFIG["access"]["ops_session"],
        }
        path = self.acct("/access/policies")
        return self.upsert(path, lambda p: p["name"] == name, path, body, f"the policy {name}")

    def application(self, fqdn: str, policy_id: str, session: str) -> None:
        body = {
            "name": fqdn,
            "domain": fqdn,
            "type": "self_hosted",
            "session_duration": session,
            "app_launcher_visible": False,
            "policies": [{"id": policy_id, "precedence": 1}],
        }
        path = self.acct("/access/apps")
        self.upsert(path, lambda a: a.get("domain") == fqdn, path, body, f"the application {fqdn}")

    def token_of(self, tunnel_id: str, name: str) -> str:
        self.plan.show(
            f"cloudflare GET {self.acct(f'/cfd_tunnel/{tunnel_id}/token')} (the result is kept in memory)"
        )
        if self.plan.dry_run:
            return placeholder(f"token of {name}")
        return self.plan.secret(self.send("GET", self.acct(f"/cfd_tunnel/{tunnel_id}/token")))


def fqdn(host: str, zone: str) -> str:
    return f"{host}.{zone}"


def cloudflare_client(plan: Plan, args) -> Cloudflare:
    for flag in ("cf_account_id", "cf_zone_id", "zone"):
        if not getattr(args, flag):
            raise Refused(f"--{flag.replace('_', '-')} is required")
    token = secret_file(plan, args.cf_token_file, "the Cloudflare API token")
    return Cloudflare(plan, token, args.cf_account_id, args.cf_zone_id)


def step_cloudflare(plan: Plan, args) -> dict[str, str]:
    """Both tunnels, remotely configured; their hostnames; one Access policy for the operators, and
    one Access application per hostname, so every public name answers only after Access."""
    if not args.access_email:
        raise Refused("--access-email is required (repeat it for each operator)")
    cf = cloudflare_client(plan, args)
    plan.say("Cloudflare: tunnels, routes, DNS and Access")
    policy = cf.policy(CONFIG["access"]["policy"], args.access_email)
    ids = {}
    for kind in ("ops", "app"):
        tunnel = CONFIG["tunnels"][kind]
        ids[kind] = cf.tunnel(tunnel["name"])
        cf.routes(ids[kind], tunnel["routes"], args.zone)
        session = CONFIG["access"][f"{kind}_session"]
        for host in tunnel["routes"]:
            cf.application(fqdn(host, args.zone), policy, session)
            cf.dns(fqdn(host, args.zone), ids[kind])
    return ids


def md5_fingerprint(public_key: str) -> str:
    blob = base64.b64decode(public_key.split()[1])
    digest = hashlib.md5(blob, usedforsecurity=False).hexdigest()
    return ":".join(digest[i : i + 2] for i in range(0, len(digest), 2))


def firewall_rules(direction: str) -> str:
    """firewall.json's rules in doctl's `--inbound-rules` or `--outbound-rules` syntax."""
    rendered = []
    for rule in FIREWALL[direction]:
        parts = [f"protocol:{rule['protocol']}"]
        if rule["protocol"] != "icmp":
            parts.append(f"ports:{rule['ports']}")
        peers = rule["sources" if direction == "inbound" else "destinations"]
        parts += [f"tag:{t}" for t in peers.get("tags", [])]
        parts += [f"address:{a}" for a in peers.get("addresses", [])]
        rendered.append(",".join(parts))
    return " ".join(rendered)


def render_user_data(role: str, public_key: str, volume: str, ops_token: str, server_ip: str) -> str:
    """The node's cloud-init: the admin user (keys only), the volume at /var/lib/rancher, k3s and,
    on the server, cloudflared, each by version and checked by SHA-256, the units, and the k3s
    config. Nothing starts k3s; `nodes` does. JSON is YAML, so this is a #cloud-config document."""

    def text(name: str) -> str:
        return (NODE / name).read_text()

    def fetch(binary: str, cfg: dict) -> str:
        """One command a line, so `set -e` stops at a failed download or a checksum mismatch."""
        tmp = f"/tmp/{binary}.download"
        return "\n".join(
            [
                f"curl -sSfL --retry 5 -o {tmp} '{cfg['url']}'",
                f"echo '{cfg['sha256']}  {tmp}' | sha256sum --check --strict",
                f"install -m 0755 {tmp} /usr/local/bin/{binary}",
                f"rm -f {tmp}",
            ]
        )

    files = [
        {
            "path": "/etc/ssh/sshd_config.d/10-cell1.conf",
            "permissions": "0644",
            "content": text("sshd-cell1.conf"),
        },
        {
            "path": "/etc/systemd/system/k3s.service",
            "permissions": "0644",
            "content": text("k3s.service").replace("__K3S_ROLE__", role),
        },
        {
            "path": "/etc/systemd/system/cell1-metadata-block.service",
            "permissions": "0644",
            "content": text("cell1-metadata-block.service"),
        },
    ]
    for script in ("cell1-metadata-block", "cell1-k3s-config", "cell1-k3s-start", "cell1-token"):
        files.append({"path": f"/usr/local/sbin/{script}", "permissions": "0755", "content": text(script)})
    commands = ["systemctl daemon-reload", "systemctl restart ssh"]
    if role == "server":
        files.append(
            {
                "path": "/etc/systemd/system/cloudflared-ops.service",
                "permissions": "0644",
                "content": text("cloudflared-ops.service"),
            }
        )
        files.append(
            {
                "path": "/etc/cell1/credentials/ops-tunnel-token",
                "permissions": "0600",
                "owner": "root:root",
                "content": ops_token,
            }
        )
        # The ops tunnel first: it is the only way in, so a later failure can still be read.
        commands += [
            fetch("cloudflared", CONFIG["cloudflared"]),
            "chmod 0700 /etc/cell1 /etc/cell1/credentials",
            "systemctl daemon-reload",
            "systemctl enable --now cloudflared-ops.service",
        ]
    commands += [
        fetch("k3s", CONFIG["k3s"]),
        "ln -sf /usr/local/bin/k3s /usr/local/bin/kubectl",
        "systemctl daemon-reload",
        "systemctl enable --now cell1-metadata-block.service",
        f"/usr/local/sbin/cell1-k3s-config {role}" + (f" {server_ip}" if role == "agent" else ""),
    ]
    files.append(
        {
            "path": "/usr/local/sbin/cell1-firstboot",
            "permissions": "0755",
            "content": "#!/bin/sh\n# cloud-init's one command: any failure stops it and fails cloud-init.\n"
            + "set -eu\n"
            + "".join(f"{c}\n" for c in commands),
        }
    )
    document = {
        "disable_root": True,
        "ssh_pwauth": False,
        "users": [
            {
                "name": CONFIG["admin_user"],
                "groups": ["sudo"],
                "shell": "/bin/bash",
                "sudo": "ALL=(ALL) NOPASSWD:ALL",
                "lock_passwd": True,
                "ssh_authorized_keys": [public_key],
            }
        ],
        "mounts": [
            [
                f"/dev/disk/by-id/scsi-0DO_Volume_{volume}",
                "/var/lib/rancher",
                "ext4",
                "defaults,nofail,discard,noatime",
                "0",
                "2",
            ]
        ],
        "package_update": False,
        "write_files": files,
        "runcmd": ["/usr/local/sbin/cell1-firstboot"],
    }
    return "#cloud-config\n" + json.dumps(document, indent=2) + "\n"


def write_private(path: Path, content: str) -> None:
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.unlink(missing_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write(content)


def step_droplets(plan: Plan, args) -> None:
    """The tag and the firewall first, so no droplet is ever up without it; then the VPC, the SSH
    key, the volumes, the server and, once its private address is known, the agent."""
    if not args.ssh_public_key:
        raise Refused("--ssh-public-key is required")
    try:
        public_key = Path(args.ssh_public_key).expanduser().read_text().strip()
    except OSError as error:
        raise Refused(f"{args.ssh_public_key}: {error.strerror}") from None
    if public_key.split()[:1] not in (["ssh-ed25519"], ["ecdsa-sha2-nistp256"], ["ssh-rsa"]):
        raise Refused(f"{args.ssh_public_key} is not an SSH public key")
    cf = cloudflare_client(plan, args)
    tag, region = CONFIG["tag"], CONFIG["region"]
    plan.say("DigitalOcean: the account doctl is signed in to")
    plan.run(["doctl", "account", "get", "--format", "Email,Status", "--no-header"])

    plan.say("The tag and the firewall, before any droplet")
    tags = plan.lines(["doctl", "compute", "tag", "list", "--format", "Name", "--no-header"], "tags")
    if [tag] not in tags:
        plan.run(["doctl", "compute", "tag", "create", tag])
    inbound, outbound = firewall_rules("inbound"), firewall_rules("outbound")
    found = plan.lines(
        ["doctl", "compute", "firewall", "list", "--format", "ID,Name", "--no-header"], "firewalls"
    )
    match = [r[0] for r in found if r[1:] == [CONFIG["firewall"]]]
    rules = ["--tag-names", tag, "--inbound-rules", inbound, "--outbound-rules", outbound]
    if match:
        plan.run(["doctl", "compute", "firewall", "update", match[0], "--name", CONFIG["firewall"], *rules])
    else:
        plan.run(["doctl", "compute", "firewall", "create", "--name", CONFIG["firewall"], *rules])

    plan.say("The VPC")
    found = plan.lines(["doctl", "vpcs", "list", "--format", "ID,Name", "--no-header"], "vpcs")
    vpc = next((r[0] for r in found if r[1:] == [CONFIG["vpc"]["name"]]), None)
    if vpc is None:
        vpc = plan.run(
            [
                "doctl",
                "vpcs",
                "create",
                "--name",
                CONFIG["vpc"]["name"],
                "--region",
                region,
                "--ip-range",
                CONFIG["vpc"]["ip_range"],
                "--format",
                "ID",
                "--no-header",
            ],
            capture=True,
            result="vpc id",
        )

    plan.say("The admin SSH key")
    fingerprint = md5_fingerprint(public_key)
    found = plan.lines(
        ["doctl", "compute", "ssh-key", "list", "--format", "FingerPrint", "--no-header"], "keys"
    )
    if [fingerprint] not in found:
        plan.run(
            [
                "doctl",
                "compute",
                "ssh-key",
                "import",
                CONFIG["ssh_key_name"],
                "--public-key-file",
                args.ssh_public_key,
            ]
        )

    plan.say("The volumes")
    found = plan.lines(
        ["doctl", "compute", "volume", "list", "--format", "ID,Name", "--no-header"], "volumes"
    )
    volumes = {r[1]: r[0] for r in found if len(r) == 2}
    for node in CONFIG["nodes"]:
        if node["volume"] not in volumes:
            volumes[node["volume"]] = plan.run(
                [
                    "doctl",
                    "compute",
                    "volume",
                    "create",
                    node["volume"],
                    "--region",
                    region,
                    "--size",
                    CONFIG["volume_size"],
                    "--fs-type",
                    "ext4",
                    "--tag",
                    tag,
                    "--desc",
                    f"{node['name']} k3s data",
                    "--format",
                    "ID",
                    "--no-header",
                ],
                capture=True,
                result=f"volume id of {node['volume']}",
            )

    plan.say("The droplets, the server first")
    found = plan.lines(
        [
            "doctl",
            "compute",
            "droplet",
            "list",
            "--tag-name",
            tag,
            "--format",
            "Name,PrivateIPv4",
            "--no-header",
        ],
        "droplets",
    )
    up = {r[0]: (r[1] if len(r) > 1 else "") for r in found if r}
    ops_token = ""
    server_ip = up.get(CONFIG["nodes"][0]["name"], "")
    for node in CONFIG["nodes"]:
        if node["name"] in up:
            continue
        if node["role"] == "agent" and not server_ip and not plan.dry_run:
            raise Refused("cell1-a has no private address yet; run `droplets` again in a minute")
        if node["role"] == "server" and not ops_token:
            ops_id = cf.tunnel(CONFIG["tunnels"]["ops"]["name"])
            ops_token = cf.token_of(ops_id, CONFIG["tunnels"]["ops"]["name"])
        user_data = render_user_data(node["role"], public_key, node["volume"], ops_token, server_ip)
        path = STATE / f"user-data-{node['name']}"
        plan.show(f"write {path} (0600, {len(user_data)} bytes; deleted once the droplet is created)")
        if not plan.dry_run:
            write_private(path, user_data)
        try:
            row = plan.run(
                [
                    "doctl",
                    "compute",
                    "droplet",
                    "create",
                    node["name"],
                    "--region",
                    region,
                    "--size",
                    CONFIG["size"],
                    "--image",
                    CONFIG["image"],
                    "--vpc-uuid",
                    vpc,
                    "--ssh-keys",
                    fingerprint,
                    "--tag-names",
                    tag,
                    "--volumes",
                    volumes[node["volume"]],
                    "--user-data-file",
                    str(path),
                    "--wait",
                    "--format",
                    "PrivateIPv4",
                    "--no-header",
                ],
                capture=True,
                result=f"private address of {node['name']}",
            )
        finally:
            if not plan.dry_run:
                path.unlink(missing_ok=True)
        if node["role"] == "server":
            server_ip = row
    plan.run(
        ["doctl", "compute", "droplet", "list", "--tag-name", tag, "--format", "Name,Status,PrivateIPv4"]
    )


def ssh_config(zone: str, agent_ip: str, identity: str) -> Path:
    admin = CONFIG["admin_user"]
    path = STATE / "ssh_config"
    known = STATE / "known_hosts"
    write_private(
        path,
        f"""Host cell1-a
  HostName {fqdn("ssh-cell1", zone)}
  User {admin}
  ProxyCommand cloudflared access ssh --hostname %h
  UserKnownHostsFile {known}
  StrictHostKeyChecking accept-new
  IdentitiesOnly yes
  IdentityFile {identity}
Host cell1-b
  HostName {agent_ip}
  User {admin}
  ProxyJump cell1-a
  UserKnownHostsFile {known}
  StrictHostKeyChecking accept-new
  IdentitiesOnly yes
  IdentityFile {identity}
""",
    )
    return path


def ready(node: str) -> str:
    """Waits, on the server, for `node` to register (up to five minutes) and then to be Ready: a
    bare `kubectl wait` fails at once on a node not yet registered."""
    return (
        "sudo sh -c 'for i in $(seq 60); do "
        f"k3s kubectl get node {node} >/dev/null 2>&1 && break; sleep 5; done; "
        f"k3s kubectl wait --for=condition=Ready node/{node} --timeout=300s'"
    )


def step_nodes(plan: Plan, args) -> None:
    if args.target == "k3d":
        k3d_nodes(plan)
        return
    if not args.zone:
        raise Refused("--zone is required")
    if not args.ssh_public_key or not args.ssh_public_key.endswith(".pub"):
        raise Refused("--ssh-public-key is required: the .pub beside the private key ssh uses")
    identity = str(Path(args.ssh_public_key).expanduser())[: -len(".pub")]
    plan.say("The agent's private address")
    found = plan.lines(
        [
            "doctl",
            "compute",
            "droplet",
            "list",
            "--tag-name",
            CONFIG["tag"],
            "--format",
            "Name,PrivateIPv4",
            "--no-header",
        ],
        "droplets",
    )
    agent_ip = dict(r for r in found if len(r) == 2).get(
        CONFIG["nodes"][1]["name"], placeholder("cell1-b private address")
    )
    config = ssh_config(args.zone, agent_ip, identity) if not plan.dry_run else STATE / "ssh_config"

    def ssh(node: str, command: str, **kw) -> str:
        return plan.run(["ssh", "-F", str(config), node, command], **kw)

    plan.say("cloud-init done on both nodes (through the ops tunnel and Access; the browser asks once)")
    for node in ("cell1-a", "cell1-b"):
        for attempt in range(60):
            try:
                ssh(node, "cloud-init status --wait >/dev/null && cloud-init status")
                break
            except Refused as refusal:
                if refusal.code != SSH_UNREACHABLE or attempt == 59:
                    raise
                time.sleep(10)
    plan.say("k3s: the server, then the agent with the server's join token")
    ssh("cell1-a", "sudo /usr/local/sbin/cell1-k3s-start")
    ssh("cell1-a", ready("cell1-a"))
    token = ssh("cell1-a", "sudo cat /etc/rancher/k3s/agent-token", capture=True, result="join token")
    plan.secret(token)
    ssh(
        "cell1-b",
        "sudo /usr/local/sbin/cell1-token agent-token",
        stdin=token.encode(),
        stdin_note="the join token",
    )
    ssh("cell1-b", "sudo /usr/local/sbin/cell1-k3s-start")
    ssh("cell1-a", ready("cell1-b"))
    plan.say("The kubeconfig, for kubectl through the ops tunnel at 127.0.0.1")
    kubeconfig = ssh("cell1-a", "sudo cat /etc/rancher/k3s/k3s.yaml", capture=True, result="kubeconfig")
    if not plan.dry_run:
        local = f"https://127.0.0.1:{CONFIG['kube_local_port']}"
        write_private(STATE / "kubeconfig", kubeconfig.replace("https://127.0.0.1:6443", local))


def k3d_nodes(plan: Plan) -> None:
    """Two k3s nodes in containers, as cell #1 has two droplets: the server and one agent."""
    k3d = CONFIG["k3d"]
    plan.say(f"k3d: the cluster {k3d['cluster']}, a server and {k3d['agents']} agent")
    exists = plan.run(["k3d", "cluster", "list", "--no-headers"], capture=True, result="clusters")
    if plan.dry_run or k3d["cluster"] not in exists.split():
        plan.run(
            [
                "k3d",
                "cluster",
                "create",
                k3d["cluster"],
                "--image",
                k3d["image"],
                "--agents",
                str(k3d["agents"]),
                "--no-lb",
                "--k3s-arg",
                "--disable=traefik@server:0",
                "--k3s-arg",
                "--disable=servicelb@server:0",
                "--kubeconfig-update-default=false",
                "--wait",
            ]
        )
    if not plan.dry_run:
        STATE.mkdir(mode=0o700, parents=True, exist_ok=True)
    plan.run(
        [
            "k3d",
            "kubeconfig",
            "write",
            k3d["cluster"],
            "--output",
            str(STATE / "kubeconfig-k3d"),
            "--overwrite",
        ]
    )


def secret_manifest(name: str, namespace: str, data: dict[str, str], kind: str = "Opaque") -> bytes:
    encoded = {k: base64.b64encode(v.encode()).decode() for k, v in data.items()}
    body = {
        "apiVersion": "v1",
        "kind": "Secret",
        "type": kind,
        "metadata": {"name": name, "namespace": namespace},
    }
    return json.dumps({**body, "data": encoded}).encode()


def fetch_chart(plan: Plan, key: str, file: str) -> Path:
    """A chart from its GitHub release, `CONFIG[key]`'s url, refused unless its SHA-256 matches."""
    chart = CONFIG[key]
    path = STATE / file
    plan.run(["curl", "-sSfL", "--retry", "5", "-o", str(path), chart["url"]])
    if not plan.dry_run:
        found = hashlib.sha256(path.read_bytes()).hexdigest()
        if found != chart["sha256"]:
            path.unlink()
            raise Refused(f"the chart {file}'s SHA-256 is {found}, not {chart['sha256']}")
    plan.show(f"sha256 {path} == {chart['sha256']}")
    return path


ARCHIVE_DESTINATION = re.compile(r"s3://[a-z0-9][a-z0-9.-]{1,61}[a-z0-9](/[A-Za-z0-9._-]+)*")
ARCHIVE_ENDPOINT = re.compile(r"https://([a-z0-9.-]+)(:([0-9]+))?")
ARCHIVE_CIDR = re.compile(r"([0-9]{1,3}\.){3}[0-9]{1,3}/(2[4-9]|3[0-2])")


def archive_cidr_ok(cidr: str) -> bool:
    """An IPv4 network written with its prefix, /24 to /32, with no host bits set and every octet
    at most 255, so never 0.0.0.0/0 nor 999.1.1.1/24."""
    if not ARCHIVE_CIDR.fullmatch(cidr):
        return False
    try:
        network = ipaddress.ip_network(cidr, strict=True)
    except ValueError:
        return False
    return network.version == 4 and 24 <= network.prefixlen <= 32


def hosted_archive(args) -> list[str]:
    """Cell #1's journal archive, as helm arguments over values/cell1-journal-db.yaml. The bucket
    is the founder's choice and spend (DEC-79, DEC-995), so nothing here names one: without the
    founder's --archive-destination, --archive-endpoint and --archive-egress-cidr, `cluster`
    refuses before it calls anything, dry run included (DEC-2006)."""
    missing = [
        flag
        for flag, value in (
            ("--archive-destination", args.archive_destination),
            ("--archive-endpoint", args.archive_endpoint),
            ("--archive-egress-cidr", args.archive_egress_cidr),
        )
        if not value
    ]
    if missing:
        raise Refused(
            "the journal's WAL archive has no destination: cell #1's journal-db needs an "
            "S3-compatible bucket the founder chooses (founder-reserved spending, DEC-79, DEC-995; "
            f"DEC-2006). Pass {', '.join(missing)} (deploy/hosted/RUNBOOK.md, gate 5)"
        )
    if not ARCHIVE_DESTINATION.fullmatch(args.archive_destination):
        raise Refused(f"--archive-destination {args.archive_destination!r} is not s3://<bucket>/<path>")
    endpoint = ARCHIVE_ENDPOINT.fullmatch(args.archive_endpoint)
    if endpoint is None:
        raise Refused(f"--archive-endpoint {args.archive_endpoint!r} is not https://<host>[:<port>]")
    for cidr in args.archive_egress_cidr:
        if not archive_cidr_ok(cidr):
            raise Refused(f"--archive-egress-cidr {cidr!r} is not an IPv4 CIDR of /24 or narrower")
    egress = {
        "cidrs": list(args.archive_egress_cidr),
        "port": int(endpoint.group(3) or 443),
        "namespace": "",
        "podSelector": {},
    }
    return [
        "--set",
        f"archive.destinationPath={args.archive_destination}",
        "--set",
        f"archive.endpointURL={args.archive_endpoint}",
        "--set-json",
        f"archive.egress={json.dumps(egress, separators=(',', ':'))}",
    ]


def k3d_archive() -> list[str]:
    """The k3d trial's journal archive: the local cell's test store, as `cargo xtask cell up` sets it."""
    return ["-f", str(HERE / "values" / "k3d-journal-archive.yaml")]


def backup_plugin(plan: Plan, helm: list[str], kubectl: list[str]) -> None:
    """CloudNativePG's Barman Cloud plugin, which archives the journal's WAL and takes its base
    backups (E21-41), with the certificates deploy/cell/charts/backup-plugin-tls makes in place of
    cert-manager's, as `cargo xtask cell up` installs it. When an upgrade remade them, the plugin
    restarts, since it reads them only at start."""

    def certificate_ends() -> str:
        ends = []
        for name in ("barman-cloud-server-tls", "barman-cloud-client-tls"):
            ends.append(
                plan.run(
                    [
                        *kubectl,
                        "get",
                        "secret",
                        name,
                        "--namespace",
                        "cnpg-system",
                        "--ignore-not-found",
                        "-o",
                        "jsonpath={.metadata.annotations.mandate\\.dev/not-after}",
                    ],
                    capture=True,
                    result=f"{name}'s end",
                )
            )
        return "" if not any(ends) else "/".join(ends)

    before = certificate_ends()
    plan.run(
        [
            *helm,
            "upgrade",
            "--install",
            "backup-plugin-tls",
            str(CELL / "charts" / "backup-plugin-tls"),
            "--namespace",
            "cnpg-system",
        ]
    )
    plugin = CONFIG["barman_plugin"]
    chart = fetch_chart(plan, "barman_plugin", "plugin-barman-cloud.tgz")
    plan.run(
        [
            *helm,
            "upgrade",
            "--install",
            "barman-cloud",
            str(chart),
            "--namespace",
            "cnpg-system",
            "--set",
            f"image.tag={plugin['image_tag']}",
            "--set",
            f"sidecarImage.tag={plugin['sidecar_tag']}",
            "--set",
            "certificate.createClientCertificate=false",
            "--set",
            "certificate.createServerCertificate=false",
            "--set",
            "certificate.createIssuer=false",
            "--wait",
            "--timeout",
            "5m",
        ]
    )
    if not plan.dry_run and before and before != certificate_ends():
        plan.run([*kubectl, "rollout", "restart", "deployment/barman-cloud", "--namespace", "cnpg-system"])
        plan.run(
            [
                *kubectl,
                "rollout",
                "status",
                "deployment/barman-cloud",
                "--namespace",
                "cnpg-system",
                "--timeout=300s",
            ]
        )


def images(plan: Plan, args) -> dict[str, str]:
    """The chart's images by digest, from the release run's multi-arch PUBLISHED list
    (`<image>@sha256:<digest>  <binary>` per line, the `published-<commit>` artifact),
    or, on k3d, the change's own `mandate.local/` build."""
    if args.target == "k3d":
        return {"mandate": args.local_image} if args.local_image else {}
    if not args.published:
        raise Refused("--published is required: the release run's PUBLISHED file")
    if plan.dry_run:
        return {
            "mandate": placeholder("ghcr.io/kunwarshivam/mandate/mandate@sha256:... from " + args.published)
        }
    refs = []
    for line in Path(args.published).read_text().splitlines():
        fields = line.split()
        if (
            len(fields) == 2
            and fields[1] == "mandate"
            and re.fullmatch(r"ghcr\.io/[a-z0-9./_-]+@sha256:[0-9a-f]{64}", fields[0])
        ):
            refs.append(fields[0])
    if len(refs) != 1:
        raise Refused(
            f"{args.published} must name the mandate image's multi-arch index by digest exactly once"
        )
    return {"mandate": refs[0]}


def open_kube_tunnel(argv: list[str]) -> subprocess.Popen:
    """`cloudflared access tcp`, kept running while kubectl and helm use it."""
    return subprocess.Popen(argv, stdout=subprocess.DEVNULL)


def require_dev_tier(plan: Plan, args) -> None:
    if args.target == "hosted" and not args.accept_dev_tier and not plan.dry_run:
        raise Refused(
            "--accept-dev-tier is required: cell #1 runs unsigned builds as `dev` (DEC-2001 item 6), "
            "only once the founder has answered DEC-971 item 7(b)"
        )


CLUSTER_SECRET_FILES = (
    ("ghcr_token_file", "the GHCR read:packages token"),
    ("cf_token_file", "the Cloudflare API token"),
    ("r2_access_key_id_file", "the R2 access key id"),
    ("r2_secret_access_key_file", "the R2 secret access key"),
    ("archive_access_key_id_file", "the archive's access key id"),
    ("archive_secret_access_key_file", "the archive's secret access key"),
)


def preflight(plan: Plan, args, order: list[str]) -> None:
    """A real hosted run that reaches `cluster` refuses before its first step, with the messages
    `cluster` would give, so `all` never buys droplets for a cluster step that cannot run: the
    dev-tier answer, the founder's archive (DEC-2006), and each secret file `cluster` reads, in its
    order. `cluster` checks them all again itself."""
    if args.target != "hosted" or plan.dry_run or "cluster" not in order:
        return
    require_dev_tier(plan, args)
    hosted_archive(args)
    for flag, what in CLUSTER_SECRET_FILES:
        secret_file(plan, getattr(args, flag), what)


def step_cluster(plan: Plan, args) -> None:
    """The same on k3d and on cell #1: the namespace and its secrets, the pull secret on the
    namespace's default service account, the CloudNativePG operator, journal-db, the workspace."""
    namespace = CONFIG["namespace"]
    hosted = args.target == "hosted"
    require_dev_tier(plan, args)
    archive = hosted_archive(args) if hosted else k3d_archive()
    kubeconfig = STATE / ("kubeconfig" if hosted else "kubeconfig-k3d")
    tunnel = None
    picked = images(plan, args)
    try:
        if hosted:
            if not args.zone:
                raise Refused("--zone is required")
            plan.say("kubectl through the ops tunnel and Access")
            argv = [
                "cloudflared",
                "access",
                "tcp",
                "--hostname",
                fqdn("kube-cell1", args.zone),
                "--url",
                f"127.0.0.1:{CONFIG['kube_local_port']}",
            ]
            plan.show(shlex.join(argv) + " &")
            if not plan.dry_run:
                tunnel = open_kube_tunnel(argv)
                time.sleep(3)
        kubectl = ["kubectl", "--kubeconfig", str(kubeconfig)]
        plan.say(f"The namespace {namespace} and its secrets, each from stdin")
        ns = {"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": namespace}}
        plan.run(
            [*kubectl, "apply", "--server-side", "--field-manager=cell1", "-f", "-"],
            stdin=json.dumps(ns).encode(),
            stdin_note="the Namespace",
        )
        names = CONFIG["secrets"]
        if hosted:
            user = args.ghcr_user or ""
            if not user:
                raise Refused(
                    "--ghcr-user is required: the GitHub account the read:packages token belongs to"
                )
            token = secret_file(plan, args.ghcr_token_file, "the GHCR read:packages token")
            auth = base64.b64encode(f"{user}:{token}".encode()).decode()
            plan.secret(auth)
            config = json.dumps({"auths": {"ghcr.io": {"auth": auth}}})
            cf = cloudflare_client(plan, args)
            app = CONFIG["tunnels"]["app"]["name"]
            tunnel_token = cf.token_of(cf.tunnel(app), app)
            key_id = secret_file(plan, args.r2_access_key_id_file, "the R2 access key id")
            key = secret_file(plan, args.r2_secret_access_key_file, "the R2 secret access key")
            if not args.r2_endpoint:
                raise Refused("--r2-endpoint is required: https://<account id>.r2.cloudflarestorage.com")
            archive_id = secret_file(plan, args.archive_access_key_id_file, "the archive's access key id")
            archive_key = secret_file(
                plan, args.archive_secret_access_key_file, "the archive's secret access key"
            )
        else:
            config = json.dumps({"auths": {}})
            tunnel_token = ""
            key_id, key = "local-cell-has-no-r2", plan.secret(secrets.token_hex(16))
        made = [
            (
                names["pull"],
                {".dockerconfigjson": config},
                "kubernetes.io/dockerconfigjson",
                "the GHCR pull secret",
            ),
            (
                names["object_store"],
                {"access-key-id": key_id, "secret-access-key": key},
                "Opaque",
                "the R2 key pair",
            ),
        ]
        if hosted:
            made.append((names["tunnel"], {"token": tunnel_token}, "Opaque", "the app tunnel's token"))
            made.append(
                (
                    names["archive"],
                    {"ACCESS_KEY_ID": archive_id, "ACCESS_SECRET_KEY": archive_key},
                    "Opaque",
                    "the journal archive's key pair",
                )
            )
        for name, data, kind, note in made:
            plan.run(
                [*kubectl, "apply", "--server-side", "--field-manager=cell1", "-f", "-"],
                stdin=secret_manifest(name, namespace, data, kind),
                stdin_note=f"Secret {namespace}/{name}: {note}",
            )
        settings = {
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {"name": CONFIG["object_store"]["config_map"], "namespace": namespace},
            "data": {
                "bucket": CONFIG["object_store"]["bucket"],
                "endpoint": args.r2_endpoint or "",
                "region": "auto",
            },
        }
        plan.run(
            [*kubectl, "apply", "--server-side", "--field-manager=cell1", "-f", "-"],
            stdin=json.dumps(settings).encode(),
            stdin_note="the object store's settings",
        )
        plan.say("The pull secret on the namespace's default service account")
        patch = json.dumps({"imagePullSecrets": [{"name": names["pull"]}]})
        for attempt in range(30):
            try:
                plan.run([*kubectl, "-n", namespace, "patch", "serviceaccount", "default", "-p", patch])
                break
            except Refused:
                if attempt == 29:
                    raise
                time.sleep(2)

        helm = ["helm", "--kubeconfig", str(kubeconfig)]
        plan.say("The CloudNativePG operator")
        chart = fetch_chart(plan, "cnpg_chart", "cloudnative-pg.tgz")
        plan.run(
            [
                *helm,
                "upgrade",
                "--install",
                "cnpg",
                str(chart),
                "--namespace",
                "cnpg-system",
                "--create-namespace",
                "--set",
                f"image.tag={CONFIG['cnpg_chart']['operator_tag']}",
                "--wait",
                "--timeout",
                "5m",
            ]
        )
        plan.say("The Barman Cloud plugin, which archives the journal's WAL and base backups")
        backup_plugin(plan, helm, kubectl)
        if not hosted:
            plan.say("The local cell's test store, the k3d trial's archive (it makes the archive's key)")
            plan.run(
                [
                    *helm,
                    "upgrade",
                    "--install",
                    "archive-store",
                    str(CELL / "charts" / "archive-store"),
                    "--namespace",
                    CONFIG["k3d"]["archive_store_namespace"],
                    "--create-namespace",
                    "--set",
                    f"clientNamespace={namespace}",
                    "--wait",
                    "--timeout",
                    "5m",
                ]
            )
        plan.say("The journal's Postgres")
        plan.run(
            [
                *helm,
                "upgrade",
                "--install",
                "journal",
                str(CELL / "charts" / "journal-db"),
                "--namespace",
                namespace,
                "-f",
                str(HERE / "values" / "cell1-journal-db.yaml"),
                *archive,
                "--wait",
                "--timeout",
                "10m",
            ]
        )
        plan.run(
            [
                *kubectl,
                "wait",
                "--namespace",
                namespace,
                "--for=condition=Ready",
                "cluster.postgresql.cnpg.io/journal",
                "--timeout=600s",
            ]
        )
        plan.say("The workspace")
        values = ["-f", str(CELL / "values" / "k3s.yaml"), "-f", str(HERE / "values" / "cell1.yaml")]
        sets = [f"images.{k}={v}" for k, v in sorted(picked.items())]
        if not hosted:
            sets += ["localImages=true", "ingress.cloudflared.enabled=false"]
            if "mandate" in picked:
                plan.run(["k3d", "image", "import", "--cluster", CONFIG["k3d"]["cluster"], picked["mandate"]])
        plan.run(
            [
                *helm,
                "upgrade",
                "--install",
                "workspace",
                str(CELL / "charts" / "workspace"),
                "--namespace",
                namespace,
                *values,
                *[a for s in sets for a in ("--set", s)],
                "--wait",
                "--wait-for-jobs",
                "--timeout",
                "10m",
            ]
        )
        plan.run([*kubectl, "get", "pods", "--all-namespaces", "-o", "wide"])
    finally:
        if tunnel is not None:
            tunnel.terminate()


def step_check(plan: Plan, args) -> None:
    """The API's public hostname must send an unauthenticated caller to Access, never to the origin."""
    if args.target == "k3d":
        plan.run(["kubectl", "--kubeconfig", str(STATE / "kubeconfig-k3d"), "get", "pods", "-A"])
        return
    if not args.zone:
        raise Refused("--zone is required")
    url = f"https://{fqdn('api', args.zone)}/"
    plan.say(f"{url} answers only through Access")
    location = plan.run(
        ["curl", "-sS", "-o", "/dev/null", "-w", "%{http_code} %{redirect_url}", url],
        capture=True,
        result="answer",
    )
    if not plan.dry_run:
        code, _, target = location.partition(" ")
        if code not in ("302", "303") or ".cloudflareaccess.com/" not in target:
            raise Refused(f"{url} answered {code} {target!r}, not a redirect to Cloudflare Access")


STEPS = {
    "cloudflare": step_cloudflare,
    "droplets": step_droplets,
    "nodes": step_nodes,
    "cluster": step_cluster,
    "check": step_check,
}


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("step", choices=[*STEPS, "all"])
    p.add_argument("--dry-run", action="store_true", help="print every command and request; call nothing")
    p.add_argument("--target", choices=["hosted", "k3d"], default="hosted")
    p.add_argument("--zone", help="the Cloudflare zone, e.g. owlhead.ai")
    p.add_argument("--cf-account-id")
    p.add_argument("--cf-zone-id")
    p.add_argument("--cf-token-file", help="a 0600 file holding the Cloudflare API token")
    p.add_argument("--access-email", action="append", help="an operator Access lets in; repeatable")
    p.add_argument("--ssh-public-key", help="the founder's SSH public key (.pub)")
    p.add_argument("--ghcr-user")
    p.add_argument("--ghcr-token-file", help="a 0600 file holding a read:packages token")
    p.add_argument("--r2-endpoint", help="https://<account id>.r2.cloudflarestorage.com")
    p.add_argument("--r2-access-key-id-file")
    p.add_argument("--r2-secret-access-key-file")
    p.add_argument(
        "--archive-destination",
        help="the journal's WAL archive, s3://<bucket>/<path>; the founder's choice (DEC-2006)",
    )
    p.add_argument("--archive-endpoint", help="the archive's S3 endpoint, https://<host>[:<port>]")
    p.add_argument(
        "--archive-egress-cidr",
        action="append",
        help="a CIDR (/24 or narrower) the archive's endpoint answers on; repeatable",
    )
    p.add_argument("--archive-access-key-id-file", help="a 0600 file holding the archive's access key id")
    p.add_argument("--archive-secret-access-key-file", help="a 0600 file holding its secret access key")
    p.add_argument(
        "--accept-dev-tier",
        action="store_true",
        help="the founder has answered DEC-971 item 7(b): cell #1 may run unsigned builds as dev",
    )
    p.add_argument("--published", help="the release run's PUBLISHED file: image@sha256 per line")
    p.add_argument(
        "--local-image",
        help="k3d only: a mandate.local/ image from `cargo xtask cell up`; without one, migrations only",
    )
    return p


def main(argv: list[str] | None = None, out=sys.stdout) -> int:
    args = parser().parse_args(argv)
    plan = Plan(args.dry_run, out)
    if args.dry_run:
        plan.say("Dry run: nothing below is called")
    if args.target == "k3d" and args.step in ("cloudflare", "droplets"):
        print(f"`{args.step}` is for cell #1 only; k3d has no Cloudflare or DigitalOcean", file=sys.stderr)
        return 2
    order = ["cloudflare", "droplets", "nodes", "cluster", "check"] if args.step == "all" else [args.step]
    if args.target == "k3d":
        order = [s for s in order if s not in ("cloudflare", "droplets")]
    try:
        preflight(plan, args, order)
        for name in order:
            STEPS[name](plan, args)
    except Refused as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
