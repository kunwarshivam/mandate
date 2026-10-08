"""Security invariants of deploy/ (the demo API host, DEC-822), checked on the committed files.

ShellCheck, typos, and gitleaks cannot see these properties, so each is asserted here, and each
assertion is shown failing on a seeded bug in `test_each_seeded_bug_is_caught`.
"""

import re
import subprocess
from pathlib import Path

import pytest

DEPLOY = Path(__file__).resolve().parents[1]
EXAMPLE_DB_URL = "postgresql://owlhead_api@localhost/owlhead?host=/var/run/postgresql"


def files() -> dict[str, str]:
    paths = [*DEPLOY.glob("*.sh"), *DEPLOY.glob("systemd/*"), DEPLOY / "postgres/10-owlhead.conf"]
    texts = {str(p.relative_to(DEPLOY)): p.read_text() for p in paths}
    texts["api.env.example"] = (DEPLOY / "api.env.example").read_text()
    return texts


def strict_mode(f: dict[str, str]) -> list[str]:
    """Every script stops on an error, an unset variable, or a failed pipe stage."""
    return [name for name, text in f.items() if name.endswith(".sh") and "\nset -euo pipefail\n" not in text]


def env_files_private(f: dict[str, str]) -> list[str]:
    """api.env and cloudflared.env are written only as root:root 0600."""
    problems = []
    for name, target in (("bootstrap.sh", "/etc/owlhead/api.env"), ("install-cloudflared.sh", '"$TOKEN_FILE"')):
        writes = [line for line in f[name].splitlines() if "put_file" in line and target in line]
        if not writes or any("0600 root:root" not in line for line in writes):
            problems.append(f"{name}: {target} not written 0600 root:root")
    if "TOKEN_FILE=/etc/owlhead/cloudflared.env" not in f["install-cloudflared.sh"]:
        problems.append("install-cloudflared.sh: the token file moved")
    if not re.search(r"chmod 0600 /etc/owlhead/api\.env", f["bootstrap.sh"]):
        problems.append("bootstrap.sh: an existing api.env is not reset to 0600")
    return problems


def api_unit_hardened(f: dict[str, str]) -> list[str]:
    """The API binds loopback by argument, cannot gain privileges, and holds no capability."""
    unit = f["systemd/owlhead-api.service"]
    lines = set(unit.splitlines())
    problems = []
    if "ExecStart=/usr/local/bin/mandate-api-server --bind 127.0.0.1:8080" not in lines:
        problems.append("ExecStart does not bind 127.0.0.1:8080 by argument")
    if re.search(r"^Environment=.*BIND", unit, re.M):
        problems.append("the bind is an Environment= line, which EnvironmentFile= overrides")
    for required in ("NoNewPrivileges=yes", "CapabilityBoundingSet=", "AmbientCapabilities=", "User=owlhead_api"):
        if required not in lines:
            problems.append(f"missing {required}")
    return problems


def postgres_local_only(f: dict[str, str]) -> list[str]:
    """PostgreSQL listens on the loopback interface only."""
    settings = re.findall(r"^\s*listen_addresses\s*=\s*(.+?)\s*$", f["postgres/10-owlhead.conf"], re.M)
    return [] if settings == ["'localhost'"] else [f"listen_addresses is {settings}"]


def api_role_minimal(f: dict[str, str]) -> list[str]:
    """owlhead_api gets CONNECT on its database and membership of the journal's app role, nothing
    more; no role is given a superuser-like attribute."""
    text = f["bootstrap.sh"]
    problems = []
    grants = re.findall(r"^\s*GRANT .*\bTO \$API_USER\b.*$", text, re.M)
    if grants != ["GRANT CONNECT ON DATABASE $DB TO $API_USER;"]:
        problems.append(f"grants to the API role: {grants}")
    created = re.findall(r"CREATE ROLE \$API_USER (.*);", text)
    if created != ["LOGIN IN ROLE mandate_journal_app"]:
        problems.append(f"the API role is created as {created}")
    if re.search(r"\b(SUPERUSER|CREATEDB|CREATEROLE|BYPASSRLS|REPLICATION)\b", text):
        problems.append("a role attribute beyond LOGIN")
    return problems


TOKEN_USES = (
    r'^\s*read -r -s -p "[^"]*" token$',
    r'^\s*token="\$\{token//\[\[:space:\]\]/\}"$',
    r'^\s*if ! \[\[ "\$token" =~ .*\]\]; then$',
    r"^\s*printf 'TUNNEL_TOKEN=%s\\n' \"\$token\" \| put_file \"\$TOKEN_FILE\" 0600 root:root$",
)


def token_never_shown(f: dict[str, str]) -> list[str]:
    """The tunnel token reaches only put_file: never echoed, printed, or passed as an argument."""
    problems = []
    for name, text in f.items():
        for line in text.splitlines():
            if re.search(r"\$\{?token\b", line) and not any(re.match(p, line) for p in TOKEN_USES):
                problems.append(f"{name}: {line.strip()}")
    return problems


def example_names_only(f: dict[str, str]) -> list[str]:
    """api.env.example holds names and comments; the database URL, which has no secret, is the only
    value."""
    problems = []
    for line in f["api.env.example"].splitlines():
        if not line or line.startswith("#"):
            continue
        name, _, value = line.partition("=")
        if not re.fullmatch(r"[A-Z][A-Z0-9_]*", name) or (value and line != f"MANDATE_API_DATABASE_URL={EXAMPLE_DB_URL}"):
            problems.append(line)
    return problems


def restore_checks_heads(f: dict[str, str]) -> list[str]:
    """The restore check compares every verified stream with its recorded head."""
    text = f["restore-check.sh"]
    ok = "FROM journal.stream_heads" in text and re.search(r'^\s*check_head "\$verified"', text, re.M)
    return [] if ok else ["restore-check.sh does not check each stream's head"]


def backup_covers_state(f: dict[str, str]) -> list[str]:
    """The nightly backup holds the database, the artifact store, and the vault."""
    text = f["backup.sh"]
    missing = [part for part in ("pg_dump", "artifacts vault") if part not in text]
    return [f"backup.sh misses {missing}"] if missing else []


def keys_pinned(f: dict[str, str]) -> list[str]:
    """Both apt repositories are trusted only after their key's fingerprint matches a pinned one."""
    problems = []
    for name, pin, compare in (
        ("bootstrap.sh", "PGDG_FINGERPRINT=B97B0AFCAA1A47F044F244A07FCC7D46ACCC4CF8", '"$found" != "$PGDG_FINGERPRINT"'),
        ("install-cloudflared.sh", "CLOUDFLARE_FINGERPRINT=CC94B39C77AE7342A68B89628A682D308D4E5E73", '"$found" != "$CLOUDFLARE_FINGERPRINT"'),
    ):
        if pin not in f[name] or compare not in f[name]:
            problems.append(f"{name}: its repository key is not checked against the pinned fingerprint")
    return problems


CHECKS = (
    keys_pinned,
    strict_mode,
    env_files_private,
    api_unit_hardened,
    postgres_local_only,
    api_role_minimal,
    token_never_shown,
    example_names_only,
    restore_checks_heads,
    backup_covers_state,
)


@pytest.mark.parametrize("check", CHECKS, ids=lambda c: c.__name__)
def test_the_committed_files_hold(check):
    assert check(files()) == []


SEEDED = (
    ("install-cloudflared.sh", '"$found" != "$CLOUDFLARE_FINGERPRINT"', '"$found" = "$found"', keys_pinned),
    ("backup.sh", "set -euo pipefail\n", "set -eu\n", strict_mode),
    ("bootstrap.sh", "/etc/owlhead/api.env 0600 root:root", "/etc/owlhead/api.env 0644 root:root", env_files_private),
    ("install-cloudflared.sh", '"$TOKEN_FILE" 0600 root:root', '"$TOKEN_FILE" 0640 root:root', env_files_private),
    ("systemd/owlhead-api.service", "--bind 127.0.0.1:8080", "--bind 0.0.0.0:8080", api_unit_hardened),
    ("systemd/owlhead-api.service", "NoNewPrivileges=yes", "NoNewPrivileges=no", api_unit_hardened),
    ("systemd/owlhead-api.service", "CapabilityBoundingSet=\n", "CapabilityBoundingSet=CAP_NET_ADMIN\n", api_unit_hardened),
    ("systemd/owlhead-api.service", "[Service]\n", "[Service]\nEnvironment=MANDATE_API_BIND=0.0.0.0:8080\n", api_unit_hardened),
    ("postgres/10-owlhead.conf", "listen_addresses = 'localhost'", "listen_addresses = '*'", postgres_local_only),
    ("bootstrap.sh", "GRANT CONNECT ON DATABASE $DB TO $API_USER;", "GRANT CONNECT ON DATABASE $DB TO $API_USER;\nGRANT ALL ON SCHEMA journal TO $API_USER;", api_role_minimal),
    ("bootstrap.sh", "LOGIN IN ROLE mandate_journal_app", "LOGIN SUPERUSER IN ROLE mandate_journal_app", api_role_minimal),
    ("install-cloudflared.sh", "  unset token\n", '  echo "token: $token"\n  unset token\n', token_never_shown),
    ("install-cloudflared.sh", "  unset token\n", '  cloudflared service install "$token"\n  unset token\n', token_never_shown),
    ("api.env.example", "ALPACA_OAUTH_CLIENT_SECRET=\n", "ALPACA_OAUTH_CLIENT_SECRET=s3cr3t\n", example_names_only),
    ("restore-check.sh", '  check_head "$verified"', '  : "$verified"', restore_checks_heads),
    ("backup.sh", "artifacts vault", "artifacts", backup_covers_state),
)


@pytest.mark.parametrize("name,old,new,check", SEEDED, ids=lambda v: v if isinstance(v, str) and len(v) < 40 else None)
def test_each_seeded_bug_is_caught(name, old, new, check):
    seeded = files()
    assert old in seeded[name], f"the seed no longer applies to {name}"
    seeded[name] = seeded[name].replace(old, new, 1)
    assert check(seeded), f"{check.__name__} missed: {new!r} in {name}"


def test_every_check_has_a_seeded_bug():
    assert {c.__name__ for c in CHECKS} == {s[3].__name__ for s in SEEDED}


HEAD = "7e4744458fb80c24b8544423bae8009571c48ecafe035ef7de24cf14611a0ade"


def check_head(verified: str, seq: str, head_hash: str) -> int:
    script = f'. "{DEPLOY}/lib.sh"; check_head "$1" ctl:ws_demo "$2" "$3"'
    return subprocess.run(["bash", "-c", script, "check", verified, seq, head_hash], capture_output=True).returncode


def test_a_stream_verified_to_its_recorded_head_passes():
    line = f"result: verified, stream ctl:ws_demo, seq 1 to 2, last hash {HEAD}"
    assert check_head(line, "2", HEAD) == 0


@pytest.mark.parametrize(
    "verified",
    [
        "result: verified, stream ctl:ws_demo, seq 1 to 1, last hash " + "d" * 64,
        "result: verified, stream ctl:ws_demo, seq 1 to 2, last hash " + "d" * 64,
        "result: verified, stream ctl:ws_two, seq 1 to 2, last hash " + HEAD,
        "",
    ],
    ids=["tail_truncated", "head_hash_differs", "another_stream", "no_result"],
)
def test_a_dump_that_lost_its_tail_fails_the_head_check(verified):
    assert check_head(verified, "2", HEAD) == 1
