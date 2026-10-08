"""Security invariants of deploy/ (the demo API host, DEC-822), checked on the committed files.

ShellCheck, typos, and gitleaks cannot see these properties, so each is asserted here, and each
assertion is shown failing on a seeded bug in `test_each_seeded_bug_is_caught`.
"""

import re
import subprocess
from pathlib import Path

import pytest

DEPLOY = Path(__file__).resolve().parents[1]
EXAMPLE_VALUES = {
    "MANDATE_API_DATABASE_URL": "postgresql://owlhead_api@localhost/owlhead?host=/var/run/postgresql",
    "MANDATE_EXECUTOR_DATABASE_URL": "postgresql://owlhead_exec@localhost/owlhead?host=/var/run/postgresql",
    "MANDATE_VAULT_DIR": "/var/lib/owlhead/vault",
}
EXAMPLES = ("api.env.example", "executor.env.example")


def files() -> dict[str, str]:
    paths = [*DEPLOY.glob("*.sh"), *DEPLOY.glob("systemd/*"), DEPLOY / "postgres/10-owlhead.conf"]
    texts = {str(p.relative_to(DEPLOY)): p.read_text() for p in paths}
    for name in EXAMPLES:
        texts[name] = (DEPLOY / name).read_text()
    return texts


def strict_mode(f: dict[str, str]) -> list[str]:
    """Every script stops on an error, an unset variable, or a failed pipe stage."""
    return [name for name, text in f.items() if name.endswith(".sh") and "\nset -euo pipefail\n" not in text]


def env_files_private(f: dict[str, str]) -> list[str]:
    """api.env, executor.env, and cloudflared.env are written only as root:root 0600."""
    problems = []
    for name, target in (("bootstrap.sh", '"/etc/owlhead/$name.env"'), ("install-cloudflared.sh", '"$TOKEN_FILE"')):
        writes = [line for line in f[name].splitlines() if "put_file" in line and target in line]
        if not writes or any("0600 root:root" not in line for line in writes):
            problems.append(f"{name}: {target} not written 0600 root:root")
    if "for name in api executor; do" not in f["bootstrap.sh"]:
        problems.append("bootstrap.sh: not both env files")
    if "TOKEN_FILE=/etc/owlhead/cloudflared.env" not in f["install-cloudflared.sh"]:
        problems.append("install-cloudflared.sh: the token file moved")
    if 'run chmod 0600 "/etc/owlhead/$name.env"' not in f["bootstrap.sh"]:
        problems.append("bootstrap.sh: an existing env file is not reset to 0600")
    if f["set-secrets.sh"].count("install -o root -g root -m 0600") != 1:
        problems.append("set-secrets.sh: an env file is rewritten other than root:root 0600")
    return problems


HARDENING = ("NoNewPrivileges=yes", "CapabilityBoundingSet=", "AmbientCapabilities=", "ProtectSystem=strict")


def unit_problems(unit: str, required: tuple[str, ...]) -> list[str]:
    lines = set(unit.splitlines())
    problems = [f"missing {line}" for line in (*HARDENING, *required) if line not in lines]
    if re.search(r"^StateDirectory=", unit, re.M):
        problems.append("StateDirectory= would chown /var/lib/owlhead to one user (DEC-692)")
    return problems


def api_unit_hardened(f: dict[str, str]) -> list[str]:
    """The API binds loopback by argument, runs as owlhead_api, cannot gain privileges, and holds no
    capability."""
    unit = f["systemd/owlhead-api.service"]
    problems = unit_problems(
        unit,
        (
            "ExecStart=/usr/local/bin/mandate-api-server --bind 127.0.0.1:8080",
            "User=owlhead_api",
            "EnvironmentFile=/etc/owlhead/api.env",
        ),
    )
    if re.search(r"^Environment=.*BIND", unit, re.M):
        problems.append("the bind is an Environment= line, which EnvironmentFile= overrides")
    return problems


def executor_unit_hardened(f: dict[str, str]) -> list[str]:
    """The executor template runs as owlhead_exec with executor.env, the same hardening, and binds
    no port; bootstrap never enables it."""
    unit = f["systemd/owlhead-executor.service"]
    problems = unit_problems(
        unit, ("User=owlhead_exec", "EnvironmentFile=/etc/owlhead/executor.env", "SocketBindDeny=any")
    )
    if "SocketBindAllow" in unit:
        problems.append("the executor binds a port")
    if re.search(r"enable.*owlhead-executor", f["bootstrap.sh"]):
        problems.append("bootstrap.sh enables the executor")
    return problems


EXECUTOR_ONLY = ("ALPACA_OAUTH_CLIENT_SECRET", "MANDATE_VAULT_TOKEN_KEY")


def client_secret_executor_only(f: dict[str, str]) -> list[str]:
    """The Alpaca client secret and the vault's token key are the executor's alone (DEC-692,
    infrastructure §5.1): the API can neither use the client secret nor write a token the executor
    would accept."""
    problems = []
    api_fill = re.search(r'^fill "\$API_ENV".*?(?<!\\)\n', f["set-secrets.sh"], re.M | re.S)
    for name in EXECUTOR_ONLY:
        if name in f["api.env.example"]:
            problems.append(f"api.env.example names {name}")
        if f"\n{name}=\n" not in f["executor.env.example"]:
            problems.append(f"executor.env.example lacks {name}")
        if not api_fill or name in api_fill.group(0):
            problems.append(f"set-secrets.sh writes {name} to api.env")
    if "MANDATE_VAULT_KEY" in "".join(f[e] for e in EXAMPLES):
        problems.append("a single vault key again")
    return problems


def vault_layout(f: dict[str, str]) -> list[str]:
    """The vault's directories have DEC-692's owners and modes: the API can unlink but never list or
    read a token."""
    text = f["bootstrap.sh"]
    wanted = (
        'run install -d -o root -g root -m 0755 /var/lib/owlhead "$VAULT"',
        'run install -d -o "$API_USER" -g "$EXEC_USER" -m 2770 "$VAULT/pending"',
        'run install -d -o "$EXEC_USER" -g "$API_USER" -m 0730 "$VAULT/tokens"',
    )
    problems = [f"bootstrap.sh lacks: {line}" for line in wanted if line not in text.splitlines()]
    if "VAULT=/var/lib/owlhead/vault" not in text:
        problems.append("the vault moved")
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
    for role in ("API_USER", "EXEC_USER"):
        grants = re.findall(rf"^\s*GRANT .*\bTO \${role}\b.*$", text, re.M)
        if grants != [f"GRANT CONNECT ON DATABASE $DB TO ${role};"]:
            problems.append(f"grants to {role}: {grants}")
        created = re.findall(rf"CREATE ROLE \${role} (.*);", text)
        if created != ["LOGIN IN ROLE mandate_journal_app"]:
            problems.append(f"{role} is created as {created}")
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
    """The env examples hold names and comments; the database URLs and the vault directory, which
    hold no secret, are the only values."""
    problems = []
    for example in EXAMPLES:
        for line in f[example].splitlines():
            if not line or line.startswith("#"):
                continue
            name, _, value = line.partition("=")
            if not re.fullmatch(r"[A-Z][A-Z0-9_]*", name) or (value and value != EXAMPLE_VALUES.get(name)):
                problems.append(f"{example}: {line}")
    return problems


def restore_checks_heads(f: dict[str, str]) -> list[str]:
    """The restore check compares every verified stream with its recorded head."""
    text = f["restore-check.sh"]
    ok = "FROM journal.stream_heads" in text and re.search(r'^\s*check_head "\$verified"', text, re.M)
    return [] if ok else ["restore-check.sh does not check each stream's head"]


def backup_covers_state(f: dict[str, str]) -> list[str]:
    """The nightly backup holds the database, the artifact store, and the vault, and never
    /etc/owlhead (OPS-1)."""
    text = f["backup.sh"]
    problems = [f"backup.sh misses {part}" for part in ("pg_dump", '-czf "$DEST/state-$stamp.tar.gz.partial" artifacts vault') if part not in text]
    code = [line for line in text.splitlines() if not line.lstrip().startswith("#")]
    if any("/etc/owlhead" in line or "/etc" in line.split() for line in code):
        problems.append("backup.sh copies /etc/owlhead")
    return problems


def secrets_never_reported(f: dict[str, str]) -> list[str]:
    """set-secrets.sh reports an empty value by its name only, judged on the value itself: a
    base64 value ends in `=`, so matching the line's last character would print a secret."""
    text = f["set-secrets.sh"]
    problems = []
    if 'missing="$missing ${file##*/}:${line%%=*}"' not in text or 'if [ -z "${line#*=}" ]; then' not in text:
        problems.append("set-secrets.sh does not report empty values by name, judged on the value")
    shown = [
        line
        for line in text.splitlines()
        if re.search(r"(echo|printf).*\$\{?(line|values|pending_key|token_key|exec_key|session_key|client_secret|client_id|answer|value)\b", line)
        and not line.rstrip().endswith('>>"$rebuilt"')
    ]
    if shown:
        problems.append(f"set-secrets.sh shows a line or a value: {shown}")
    return problems


def units_umask(f: dict[str, str]) -> list[str]:
    """Both service units create files 0640 at most."""
    return [u for u in ("systemd/owlhead-api.service", "systemd/owlhead-executor.service") if "UMask=0027" not in f[u].splitlines()]


def no_cross_group(f: dict[str, str]) -> list[str]:
    """No service user joins the other's group: the only membership bootstrap grants is the shared
    artifact group, so the API never reads a token."""
    text = f["bootstrap.sh"]
    problems = []
    for line in text.splitlines():
        if re.search(r"\b(usermod|adduser|addgroup)\b", line):
            problems.append(line.strip())
        if "gpasswd" in line and line.strip() != 'run gpasswd --add "$user" "$ART_GROUP"':
            problems.append(line.strip())
        if "useradd" in line and "--groups" in line and '--groups "$ART_GROUP"' not in line:
            problems.append(line.strip())
    if "ART_GROUP=owlhead_art" not in text:
        problems.append("the shared group is not owlhead_art")
    return problems


def artifacts_shared(f: dict[str, str]) -> list[str]:
    """The artifact store and every directory it uses are root:owlhead_art 2770, made by bootstrap."""
    text = f["bootstrap.sh"]
    wanted = ('install -d -o root -g "$ART_GROUP" -m 2770 "${shards[@]}"', 'shards+=("$ARTIFACTS/sha256/$hi$lo")')
    return [f"bootstrap.sh lacks: {w}" for w in wanted if w not in text]


def etc_private(f: dict[str, str]) -> list[str]:
    """/etc/owlhead is made root:root 0700 wherever it is made, and nothing loosens it."""
    made = "run install -d -o root -g root -m 0700 /etc/owlhead"
    problems = []
    for name in ("bootstrap.sh", "install-cloudflared.sh"):
        lines = f[name].splitlines()
        if made not in lines:
            problems.append(f"{name} does not make /etc/owlhead 0700")
        problems += [f"{name}: {l.strip()}" for l in lines if re.search(r"(chmod|chown)\b.*/etc/owlhead/?(\s|$)", l)]
    return problems


def unit_paths(f: dict[str, str]) -> list[str]:
    """Each service may write exactly the artifact store and the vault's two directories."""
    wanted = "ReadWritePaths=/var/lib/owlhead/artifacts /var/lib/owlhead/vault/pending /var/lib/owlhead/vault/tokens"
    return [u for u in ("systemd/owlhead-api.service", "systemd/owlhead-executor.service") if [l for l in f[u].splitlines() if l.startswith("ReadWritePaths=")] != [wanted]]


def restore_strict(f: dict[str, str]) -> list[str]:
    """A restore stops at its first error, and bootstrap refuses a host without the peer rule."""
    problems = []
    if "runuser -u postgres -- pg_restore --exit-on-error -d" not in f["restore-check.sh"]:
        problems.append("restore-check.sh restores without --exit-on-error")
    hba = re.search(r'^if \[ "\$DRY_RUN" = 0 \] && \[ -z "\$\(query postgres "SELECT 1 FROM pg_hba_file_rules WHERE type = \'local\' AND \'all\' = ANY \(user_name\) AND auth_method = \'peer\'"\)" \]; then\n  echo [^\n]*\n  exit 1\nfi$', f["bootstrap.sh"], re.M)
    if not hba:
        problems.append("bootstrap.sh no longer refuses a host without the local peer rule")
    return problems


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
    units_umask,
    no_cross_group,
    artifacts_shared,
    etc_private,
    unit_paths,
    restore_strict,
    secrets_never_reported,
    keys_pinned,
    executor_unit_hardened,
    client_secret_executor_only,
    vault_layout,
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
    ("systemd/owlhead-executor.service", "UMask=0027", "UMask=0022", units_umask),
    ("bootstrap.sh", 'run gpasswd --add "$user" "$ART_GROUP"', 'run gpasswd --add "$user" "$ART_GROUP"\n    run usermod -aG owlhead_exec owlhead_api', no_cross_group),
    ("bootstrap.sh", 'run gpasswd --add "$user" "$ART_GROUP"', 'run gpasswd --add "$user" "$EXEC_USER"', no_cross_group),
    ("bootstrap.sh", '--groups "$ART_GROUP"', '--groups "$ART_GROUP,$EXEC_USER"', no_cross_group),
    ("bootstrap.sh", 'install -d -o root -g "$ART_GROUP" -m 2770 "${shards[@]}"', 'install -d -o root -g "$ART_GROUP" -m 2750 "${shards[@]}"', artifacts_shared),
    ("bootstrap.sh", "run install -d -o root -g root -m 0700 /etc/owlhead", "run install -d -o root -g root -m 0755 /etc/owlhead", etc_private),
    ("install-cloudflared.sh", "run install -d -o root -g root -m 0700 /etc/owlhead", "run install -d -o root -g root -m 0700 /etc/owlhead\nrun chmod 0755 /etc/owlhead", etc_private),
    ("systemd/owlhead-api.service", " /var/lib/owlhead/vault/tokens", "", unit_paths),
    ("restore-check.sh", "pg_restore --exit-on-error -d", "pg_restore -d", restore_strict),
    ("bootstrap.sh", 'if [ "$DRY_RUN" = 0 ] && [ -z "$(query postgres "SELECT 1 FROM pg_hba_file_rules', 'if false && [ "$DRY_RUN" = 0 ] && [ -z "$(query postgres "SELECT 1 FROM pg_hba_file_rules', restore_strict),
    ("bootstrap.sh", 'run install -d -o root -g root -m 0755 /var/lib/owlhead "$VAULT"', 'run install -d -o root -g root -m 0777 /var/lib/owlhead "$VAULT"', vault_layout),
    ("executor.env.example", "MANDATE_VAULT_TOKEN_KEY=\n", "", client_secret_executor_only),
    ("set-secrets.sh", 'MANDATE_VAULT_PENDING_KEY "$pending_key" \\\n  ALPACA_OAUTH_CLIENT_ID "$client_id"\nfill', 'MANDATE_VAULT_PENDING_KEY "$pending_key" MANDATE_VAULT_TOKEN_KEY "$token_key" \\\n  ALPACA_OAUTH_CLIENT_ID "$client_id"\nfill', client_secret_executor_only),
    ("set-secrets.sh", 'if [ -z "${line#*=}" ]; then', 'if [ "${line: -1}" = "=" ]; then', secrets_never_reported),
    ("set-secrets.sh", 'missing="$missing ${file##*/}:${line%%=*}"', 'missing="$missing ${file##*/}:${line}"', secrets_never_reported),
    ("set-secrets.sh", "unset pending_key exec_key token_key", 'echo "key: $token_key" >&2\nunset pending_key exec_key token_key', secrets_never_reported),
    ("install-cloudflared.sh", '"$found" != "$CLOUDFLARE_FINGERPRINT"', '"$found" = "$found"', keys_pinned),
    ("backup.sh", "set -euo pipefail\n", "set -eu\n", strict_mode),
    ("bootstrap.sh", '"/etc/owlhead/$name.env" 0600 root:root', '"/etc/owlhead/$name.env" 0644 root:root', env_files_private),
    ("set-secrets.sh", "install -o root -g root -m 0600", "install -o root -g root -m 0644", env_files_private),
    ("systemd/owlhead-executor.service", "User=owlhead_exec", "User=owlhead_api", executor_unit_hardened),
    ("systemd/owlhead-executor.service", "SocketBindDeny=any", "SocketBindAllow=tcp:9090\nSocketBindDeny=any", executor_unit_hardened),
    ("systemd/owlhead-executor.service", "NoNewPrivileges=yes", "NoNewPrivileges=no", executor_unit_hardened),
    ("systemd/owlhead-api.service", "UMask=0027", "StateDirectory=owlhead\nUMask=0027", api_unit_hardened),
    ("api.env.example", "ALPACA_OAUTH_CLIENT_ID=\n", "ALPACA_OAUTH_CLIENT_ID=\nALPACA_OAUTH_CLIENT_SECRET=\n", client_secret_executor_only),
    ("set-secrets.sh", '  ALPACA_OAUTH_CLIENT_ID "$client_id"\nfill "$EXEC_ENV"', '  ALPACA_OAUTH_CLIENT_ID "$client_id" ALPACA_OAUTH_CLIENT_SECRET "$client_secret"\nfill "$EXEC_ENV"', client_secret_executor_only),
    ("bootstrap.sh", '-m 0730 "$VAULT/tokens"', '-m 0770 "$VAULT/tokens"', vault_layout),
    ("bootstrap.sh", '-m 2770 "$VAULT/pending"', '-m 0770 "$VAULT/pending"', vault_layout),
    ("bootstrap.sh", '-o "$EXEC_USER" -g "$API_USER" -m 0730', '-o "$API_USER" -g "$API_USER" -m 0730', vault_layout),
    ("bootstrap.sh", "GRANT CONNECT ON DATABASE $DB TO $EXEC_USER;", "GRANT CONNECT ON DATABASE $DB TO $EXEC_USER;\nGRANT ALL ON SCHEMA journal TO $EXEC_USER;", api_role_minimal),
    ("executor.env.example", "ALPACA_OAUTH_CLIENT_SECRET=\n", "ALPACA_OAUTH_CLIENT_SECRET=s3cr3t\n", example_names_only),
    ("backup.sh", 'artifacts vault\n', 'artifacts vault /etc/owlhead\n', backup_covers_state),
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
    ("api.env.example", "MANDATE_VAULT_PENDING_KEY=\n", "MANDATE_VAULT_PENDING_KEY=c2VjcmV0\n", example_names_only),
    ("restore-check.sh", '  check_head "$verified"', '  : "$verified"', restore_checks_heads),
    ("backup.sh", '"$DEST/state-$stamp.tar.gz.partial" artifacts vault', '"$DEST/state-$stamp.tar.gz.partial" artifacts', backup_covers_state),
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
