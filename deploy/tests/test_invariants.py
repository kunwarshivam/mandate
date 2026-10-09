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
    """api.env, executor.env, and every credential file are written only as root:root 0600."""
    problems = []
    for name, target in (("bootstrap.sh", '"/etc/owlhead/$name.env"'), ("install-cloudflared.sh", '"$TOKEN_FILE"')):
        writes = [line for line in f[name].splitlines() if "put_file" in line and target in line]
        if not writes or any("0600 root:root" not in line for line in writes):
            problems.append(f"{name}: {target} not written 0600 root:root")
    if "for name in api executor; do" not in f["bootstrap.sh"]:
        problems.append("bootstrap.sh: not both env files")
    if "TOKEN_FILE=/etc/owlhead/credentials/tunnel-token" not in f["install-cloudflared.sh"]:
        problems.append("install-cloudflared.sh: the token file moved")
    if 'openssl rand -base64 32 | put_file "$CREDENTIALS/$key" 0600 root:root' not in f["set-secrets.sh"]:
        problems.append("set-secrets.sh: a key is not written root:root 0600 straight from openssl")
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


CREDENTIALS = "/etc/owlhead/credentials"
# DEC-822 item 4: exactly three kinds of secret outside the vault, each loaded only by its units.
# The vault's token key is the executor's alone (DEC-692), so the API cannot write a token the
# executor would accept.
LOADED = {
    "systemd/owlhead-api.service": ("vault-pending-key", "session-signing-key"),
    "systemd/owlhead-executor.service": ("vault-pending-key", "vault-token-key"),
    "systemd/cloudflared.service": ("tunnel-token",),
}
SECRET_NAME = re.compile(r"KEY|SECRET|TOKEN|PASSWORD")


def secrets_are_credentials(f: dict[str, str]) -> list[str]:
    """Every secret outside the vault is a credential file, never an environment variable: no env
    file names one, each unit loads exactly its own credentials with LoadCredential=, cloudflared
    reads no environment file, and the Alpaca client secret is nowhere under /etc/owlhead."""
    problems = []
    for example in EXAMPLES:
        for line in f[example].splitlines():
            if line and not line.startswith("#") and SECRET_NAME.search(line.partition("=")[0]):
                problems.append(f"{example}: {line.partition('=')[0]} is a secret in an environment file")
    for unit, names in LOADED.items():
        loaded = [l for l in f[unit].splitlines() if l.startswith("LoadCredential=")]
        wanted = [f"LoadCredential={n}:{CREDENTIALS}/{n}" for n in names]
        if loaded != wanted:
            problems.append(f"{unit} loads {loaded}, not {wanted}")
    if re.search(r"^EnvironmentFile=", f["systemd/cloudflared.service"], re.M):
        problems.append("cloudflared reads an environment file")
    if "${CREDENTIALS_DIRECTORY}/tunnel-token" not in f["systemd/cloudflared.service"]:
        problems.append("cloudflared does not read the token from its credential")
    if "KEYS=(vault-pending-key vault-token-key session-signing-key)" not in f["set-secrets.sh"]:
        problems.append("set-secrets.sh does not make exactly the three keys")
    for name, text in f.items():
        code = [l for l in text.splitlines() if not l.lstrip().startswith("#")]
        if any("cloudflared.env" in l or "ALPACA_OAUTH_CLIENT_SECRET" in l for l in code):
            problems.append(f"{name}: a secret back in an environment file")
    fills = [l for l in f["set-secrets.sh"].splitlines() if l.startswith("fill ")]
    if any(SECRET_NAME.search(l) for l in fills):
        problems.append("set-secrets.sh fills a secret into an env file")
    if '"$answer" | systemd-run' not in f["set-secrets.sh"]:
        problems.append("set-secrets.sh does not pipe the client secret to the vault import on stdin")
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
    r"^\s*printf '%s' \"\$token\" \| put_file \"\$TOKEN_FILE\" 0600 root:root$",
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
    if 'problems="$problems ${file##*/}:${line%%=*}(empty)"' not in text or 'if [ -z "${line#*=}" ]; then' not in text:
        problems.append("set-secrets.sh does not report empty values by name, judged on the value")
    shown = [
        line
        for line in text.splitlines()
        if re.search(r"(echo|printf).*\$\{?(line|values|client_id|answer|value)\b", line)
        and not line.rstrip().endswith('>>"$rebuilt"')
        and line.strip() != 'printf \'%s\' "$answer" | systemd-run --quiet --wait --pipe --collect \\'
    ]
    if shown:
        problems.append(f"set-secrets.sh shows a line or a value: {shown}")
    return problems


def no_side_doors(f: dict[str, str]) -> list[str]:
    """Nothing changes the vault's or the state directory's owners, modes, or ACLs outside
    bootstrap's `install -d` lines; no unit adds a group; nothing writes /etc/group."""
    problems = []
    for name, text in f.items():
        for line in text.splitlines():
            if line.lstrip().startswith("#"):
                continue
            if re.search(r"\b(chmod|chown|chgrp|setfacl)\b", line) and re.search(r"\$VAULT|/var/lib/owlhead|tokens|pending", line):
                problems.append(f"{name}: {line.strip()}")
            if re.search(r"/etc/(group|gshadow)\b", line):
                problems.append(f"{name}: {line.strip()}")
        if name.startswith("systemd/") and re.search(r"^SupplementaryGroups=", text, re.M):
            problems.append(f"{name}: SupplementaryGroups=")
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
        admin = line.strip() == 'run useradd --create-home --shell /bin/bash --groups sudo "$ADMIN_USER"'
        if "useradd" in line and "--groups" in line and '--groups "$ART_GROUP"' not in line and not admin:
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
    """/etc/owlhead and its credentials/ are made root:root 0700 wherever they are made, and nothing
    loosens them."""
    made = ("run install -d -o root -g root -m 0700 /etc/owlhead", f"run install -d -o root -g root -m 0700 {CREDENTIALS}")
    problems = []
    for name in ("bootstrap.sh", "install-cloudflared.sh"):
        lines = f[name].splitlines()
        problems += [f"{name} does not make {m.split()[-1]} 0700" for m in made if m not in lines]
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


SERVICE_UNITS = (
    "systemd/owlhead-api.service",
    "systemd/owlhead-executor.service",
    "systemd/cloudflared.service",
    "systemd/owlhead-backup.service",
    "systemd/owlhead-backup-failed.service",
)


def egress_denied(f: dict[str, str]) -> list[str]:
    """DEC-822 item 5: every service unit denies all outbound traffic and allows at most loopback,
    never a wide range; the units that need a remote host get it from allow-egress.sh's drop-in,
    which only ever adds `IPAddressAllow=` lines and never lifts the deny."""
    problems = []
    unit_files = [name for name in f if name.startswith("systemd/") and name.endswith(".service")]
    if sorted(unit_files) != sorted(SERVICE_UNITS):
        problems.append(f"a service unit this check does not know: {sorted(set(unit_files) ^ set(SERVICE_UNITS))}")
    for name in unit_files:
        lines = f[name].splitlines()
        if lines.count("IPAddressDeny=any") != 1:
            problems.append(f"{name} lacks IPAddressDeny=any")
        allowed = [l for l in lines if l.startswith("IPAddressAllow=")]
        if any(l != "IPAddressAllow=localhost" for l in allowed):
            problems.append(f"{name} allows more than loopback: {allowed}")
        if re.search(r"^IPAddressDeny=\s*$", f[name], re.M):
            problems.append(f"{name} resets IPAddressDeny=")
    script = f["allow-egress.sh"]
    if "IPAddressDeny" in "\n".join(l for l in script.splitlines() if not l.lstrip().startswith("#")):
        problems.append("allow-egress.sh touches IPAddressDeny")
    if script.count("IPAddressAllow=") != 1 or "[Service]" not in script:
        problems.append("allow-egress.sh writes something other than IPAddressAllow lines")
    if "owlhead-api | owlhead-executor | cloudflared)" not in script:
        problems.append("allow-egress.sh names units other than the three that need a host")
    return problems


REJECTED = {
    "0.0.0.0": "0.0.0.0/8", "0.1.2.3": "0.0.0.0/8", "127.0.0.1": "127.0.0.0/8", "127.255.255.254": "127.0.0.0/8",
    "10.0.0.1": "10.0.0.0/8", "172.16.0.1": "172.16.0.0/12", "172.31.255.255": "172.16.0.0/12",
    "192.168.1.1": "192.168.0.0/16", "169.254.169.254": "169.254.0.0/16", "100.64.0.1": "100.64.0.0/10",
    "100.127.255.255": "100.64.0.0/10", "224.0.0.1": "224.0.0.0/4", "239.255.255.255": "224.0.0.0/4",
    "240.0.0.1": "240.0.0.0/4", "255.255.255.255": "240.0.0.0/4",
    "::": "unspecified", "::1": "loopback", "fe80::1": "fe80::/10", "febf::1": "fe80::/10",
    "fc00::1": "fc00::/7", "fd12:3456::1": "fc00::/7", "FD00::1": "fc00::/7", "ff02::1": "ff00::/8",
    "::ffff:10.0.0.1": "IPv4-mapped", "::ffff:8.8.8.8": "IPv4-mapped",
}
GLOBAL = ["1.1.1.1", "8.8.8.8", "100.63.255.255", "100.128.0.1", "172.15.255.255", "172.32.0.1", "169.253.0.1",
          "192.167.1.1", "223.255.255.255", "11.0.0.1", "126.0.0.1", "128.0.0.1", "2606:4700::1111", "2001:db8::1"]


def run_filter(script: str, getent_output: str) -> subprocess.CompletedProcess:
    """Runs the script's own address filter and host loop with `getent` replaced by a fixed answer."""
    start, end = script.index("rejection_reason() {"), script.index('dropin="/etc')
    harness = (
        "set -euo pipefail\n"
        f"getent() {{ printf '%s' \"$GETENT_OUT\"; }}\n"
        "args=(unit example.test)\n"
        f"{script[start:end]}\n"
        "printf 'ALLOW:%s\\n' \"$allow\"\n"
    )
    return subprocess.run(["bash", "-c", harness], capture_output=True, text=True, env={"GETENT_OUT": getent_output, "PATH": "/usr/bin:/bin"})


def egress_rejects_non_global(f: dict[str, str]) -> list[str]:
    """DEC-822 item 5: a poisoned DNS answer cannot allow an internal address. Every non-global
    class is rejected with its address and reason named, a host that resolves only to such
    addresses exits 1 having produced no allow list, and global addresses pass."""
    script = f["allow-egress.sh"]
    problems = []
    for address, why in REJECTED.items():
        done = run_filter(script, f"{address} STREAM example.test\n")
        if done.returncode != 1 or "ALLOW:" in done.stdout:
            problems.append(f"{address} was not refused: exit {done.returncode}")
        if address not in done.stderr or "rejected:" not in done.stderr:
            problems.append(f"{address} was refused without naming it and the reason: {done.stderr!r}")
    for address in GLOBAL:
        done = run_filter(script, f"{address} STREAM example.test\n")
        if done.returncode != 0 or address not in done.stdout:
            problems.append(f"{address} is global but was not allowed: {done.stderr!r}")
    mixed = run_filter(script, "10.0.0.1 STREAM a\n1.1.1.1 STREAM a\n")
    if mixed.returncode != 0 or "10.0.0.1" in mixed.stdout or "1.1.1.1" not in mixed.stdout:
        problems.append(f"a mixed answer did not keep only the global address: {mixed.stdout!r}")
    if run_filter(script, "").returncode != 1:
        problems.append("an empty answer did not exit 1")
    return problems


def sshd_hardened(f: dict[str, str]) -> list[str]:
    """DEC-822 item 7: bootstrap writes an sshd drop-in that wins over cloud-init's (a 00- name) with
    keys only, no root login, and one allowed admin user; it creates that user with a key first,
    validates the file, reloads sshd, and reads the effective settings back."""
    text = f["bootstrap.sh"]
    drop = re.search(r"put_file \"\$SSHD_DROPIN\" 0644 root:root <<SSHD\n(.*?)\nSSHD\n", text, re.S)
    settings = drop.group(1).splitlines() if drop else []
    wanted = (
        "PasswordAuthentication no",
        "KbdInteractiveAuthentication no",
        "AuthenticationMethods publickey",
        "PermitRootLogin no",
        "AllowUsers $ADMIN_USER",
    )
    problems = [f"sshd drop-in lacks: {w}" for w in wanted if w not in settings]
    problems += [f"sshd drop-in loosens: {l}" for l in settings if re.match(r"(PasswordAuthentication|PermitRootLogin|PermitEmptyPasswords|PermitUserEnvironment)\s+(yes|without-password|prohibit-password)", l)]
    for line in (
        "SSHD_DROPIN=/etc/ssh/sshd_config.d/00-owlhead.conf",
        "ADMIN_USER=owlhead_admin",
        "run sshd -t",
        "run systemctl try-reload-or-restart ssh",
        'for setting in "passwordauthentication no" "permitrootlogin no" "kbdinteractiveauthentication no"; do',
    ):
        if line not in [l.strip() for l in text.splitlines()]:
            problems.append(f"bootstrap.sh lacks: {line}")
    keys_check = text.find('echo "$ADMIN_KEYS is empty')
    if keys_check == -1 or text.find("put_file \"$SSHD_DROPIN\"") < keys_check:
        problems.append("bootstrap.sh writes the sshd drop-in before it checks the admin has a key")
    return problems


CHECKS = (
    egress_denied,
    egress_rejects_non_global,
    sshd_hardened,
    no_side_doors,
    units_umask,
    no_cross_group,
    artifacts_shared,
    etc_private,
    unit_paths,
    restore_strict,
    secrets_never_reported,
    keys_pinned,
    executor_unit_hardened,
    secrets_are_credentials,
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
    ("systemd/owlhead-api.service", "IPAddressDeny=any\n", "", egress_denied),
    ("systemd/owlhead-executor.service", "IPAddressDeny=any\n", "IPAddressDeny=\n", egress_denied),
    ("systemd/cloudflared.service", "IPAddressAllow=localhost\n", "IPAddressAllow=localhost\nIPAddressAllow=0.0.0.0/0\n", egress_denied),
    ("systemd/owlhead-backup.service", "IPAddressDeny=any\n", "", egress_denied),
    ("systemd/owlhead-backup-failed.service", "IPAddressDeny=any\n", "", egress_denied),
    ("allow-egress.sh", "  printf '[Service]\\n'\n", "  printf '[Service]\\nIPAddressDeny=\\n'\n", egress_denied),
    ("allow-egress.sh", "owlhead-api | owlhead-executor | cloudflared)", "owlhead-api | owlhead-executor | cloudflared | owlhead-backup)", egress_denied),
    ("allow-egress.sh", '  elif [ "$a" -eq 10 ]; then', '  elif [ "$a" -eq 11 ]; then', egress_rejects_non_global),
    ("allow-egress.sh", '  if [ "$a" -eq 0 ]; then', '  if [ "$a" -eq 255 ]; then', egress_rejects_non_global),
    ("allow-egress.sh", '  elif [ "$a" -eq 127 ]; then', '  elif [ "$a" -eq 128 ]; then', egress_rejects_non_global),
    ("allow-egress.sh", '[ "$b" -le 127 ]', '[ "$b" -le 100 ]', egress_rejects_non_global),
    ("allow-egress.sh", '[ "$b" -eq 254 ]', '[ "$b" -eq 253 ]', egress_rejects_non_global),
    ("allow-egress.sh", '[ "$b" -le 31 ]', '[ "$b" -le 30 ]', egress_rejects_non_global),
    ("allow-egress.sh", '[ "$b" -eq 168 ]', '[ "$b" -eq 169 ]', egress_rejects_non_global),
    ("allow-egress.sh", '[ "$a" -le 239 ]', '[ "$a" -le 230 ]', egress_rejects_non_global),
    ("allow-egress.sh", 'elif [ "$a" -ge 240 ]', 'elif [ "$a" -ge 241 ]', egress_rejects_non_global),
    ("allow-egress.sh", "      ::1) echo", "      ::2) echo", egress_rejects_non_global),
    ("allow-egress.sh", "      ::) echo", "      ::0) echo", egress_rejects_non_global),
    ("allow-egress.sh", "      ::ffff:*) echo", "      ::fffe:*) echo", egress_rejects_non_global),
    ("allow-egress.sh", "      fe[89ab]*)", "      fe[89a]*)", egress_rejects_non_global),
    ("allow-egress.sh", "      f[cd]*)", "      fc*)", egress_rejects_non_global),
    ("allow-egress.sh", "      ff*)", "      fe*)", egress_rejects_non_global),
    ("allow-egress.sh", "tr 'A-F' 'a-f'", "cat", egress_rejects_non_global),
    ("allow-egress.sh", '  if [ -z "$usable" ]; then\n    echo "$host resolves to no global address; nothing written" >&2\n    exit 1', '  if [ -z "$usable" ]; then\n    echo "$host resolves to no global address; nothing written" >&2\n    exit 0', egress_rejects_non_global),
    ("allow-egress.sh", 'rejected: $reason" >&2', 'rejected" >&2', egress_rejects_non_global),
    ("bootstrap.sh", "PasswordAuthentication no\n", "PasswordAuthentication yes\n", sshd_hardened),
    ("bootstrap.sh", "PermitRootLogin no\n", "PermitRootLogin prohibit-password\n", sshd_hardened),
    ("bootstrap.sh", "AllowUsers $ADMIN_USER\n", "", sshd_hardened),
    ("bootstrap.sh", "AuthenticationMethods publickey\n", "", sshd_hardened),
    ("bootstrap.sh", "SSHD_DROPIN=/etc/ssh/sshd_config.d/00-owlhead.conf", "SSHD_DROPIN=/etc/ssh/sshd_config.d/99-owlhead.conf", sshd_hardened),
    ("bootstrap.sh", "run sshd -t\n", "", sshd_hardened),
    ("bootstrap.sh", 'for setting in "passwordauthentication no" "permitrootlogin no" "kbdinteractiveauthentication no"; do', 'for setting in "passwordauthentication no"; do', sshd_hardened),
    ("bootstrap.sh", 'run install -d -o "$EXEC_USER" -g "$API_USER" -m 0730 "$VAULT/tokens"', 'run install -d -o "$EXEC_USER" -g "$API_USER" -m 0730 "$VAULT/tokens"\nrun chmod 0777 "$VAULT"', no_side_doors),
    ("bootstrap.sh", 'run install -d -o "$EXEC_USER" -g "$API_USER" -m 0730 "$VAULT/tokens"', 'run install -d -o "$EXEC_USER" -g "$API_USER" -m 0730 "$VAULT/tokens"\nrun chown -R owlhead_api /var/lib/owlhead', no_side_doors),
    ("bootstrap.sh", 'run install -d -o "$EXEC_USER" -g "$API_USER" -m 0730 "$VAULT/tokens"', 'run install -d -o "$EXEC_USER" -g "$API_USER" -m 0730 "$VAULT/tokens"\nrun setfacl -m u:owlhead_api:r "$VAULT/tokens"', no_side_doors),
    ("systemd/owlhead-api.service", "User=owlhead_api\n", "User=owlhead_api\nSupplementaryGroups=owlhead_exec\n", no_side_doors),
    ("systemd/owlhead-executor.service", "User=owlhead_exec\n", "User=owlhead_exec\nSupplementaryGroups=owlhead_api\n", no_side_doors),
    ("bootstrap.sh", 'run groupadd --system "$ART_GROUP"', 'run groupadd --system "$ART_GROUP"\n  sed -i "s/^owlhead_exec:x:\\([0-9]*\\):/&owlhead_api/" /etc/group', no_side_doors),
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
    ("systemd/owlhead-api.service", "LoadCredential=session-signing-key:/etc/owlhead/credentials/session-signing-key\n", "LoadCredential=session-signing-key:/etc/owlhead/credentials/session-signing-key\nLoadCredential=vault-token-key:/etc/owlhead/credentials/vault-token-key\n", secrets_are_credentials),
    ("systemd/owlhead-executor.service", "LoadCredential=vault-token-key:/etc/owlhead/credentials/vault-token-key\n", "", secrets_are_credentials),
    ("systemd/cloudflared.service", "LoadCredential=tunnel-token", "EnvironmentFile=/etc/owlhead/cloudflared.env\nLoadCredential=tunnel-token", secrets_are_credentials),
    ("api.env.example", "ALPACA_OAUTH_CLIENT_ID=\n", "ALPACA_OAUTH_CLIENT_ID=\nMANDATE_API_SESSION_SIGNING_KEY=\n", secrets_are_credentials),
    ("executor.env.example", "ALPACA_OAUTH_CLIENT_ID=\n", "ALPACA_OAUTH_CLIENT_ID=\nMANDATE_VAULT_TOKEN_KEY=\n", secrets_are_credentials),
    ("set-secrets.sh", 'fill "$EXEC_ENV" ALPACA_OAUTH_CLIENT_ID "$client_id"', 'fill "$EXEC_ENV" ALPACA_OAUTH_CLIENT_ID "$client_id" ALPACA_OAUTH_CLIENT_SECRET "$answer"', secrets_are_credentials),
    ("set-secrets.sh", "KEYS=(vault-pending-key vault-token-key session-signing-key)", "KEYS=(vault-pending-key vault-token-key session-signing-key alpaca-client-secret)", secrets_are_credentials),
    ("set-secrets.sh", 'if [ -z "${line#*=}" ]; then', 'if [ "${line: -1}" = "=" ]; then', secrets_never_reported),
    ("set-secrets.sh", 'problems="$problems ${file##*/}:${line%%=*}(empty)"', 'problems="$problems ${file##*/}:${line}(empty)"', secrets_never_reported),
    ("set-secrets.sh", "unset client_id\n", 'echo "id: $client_id" >&2\nunset client_id\n', secrets_never_reported),
    ("set-secrets.sh", '  ask "Alpaca OAuth client secret (not echoed): " 1\n', '  ask "Alpaca OAuth client secret (not echoed): " 1\n  echo "secret: $answer" >&2\n', secrets_never_reported),
    ("bootstrap.sh", "run install -d -o root -g root -m 0700 /etc/owlhead/credentials", "run install -d -o root -g root -m 0755 /etc/owlhead/credentials", etc_private),
    ("set-secrets.sh", '| put_file "$CREDENTIALS/$key" 0600 root:root', '| put_file "$CREDENTIALS/$key" 0640 root:root', env_files_private),
    ("install-cloudflared.sh", '"$found" != "$CLOUDFLARE_FINGERPRINT"', '"$found" = "$found"', keys_pinned),
    ("backup.sh", "set -euo pipefail\n", "set -eu\n", strict_mode),
    ("bootstrap.sh", '"/etc/owlhead/$name.env" 0600 root:root', '"/etc/owlhead/$name.env" 0644 root:root', env_files_private),
    ("set-secrets.sh", "install -o root -g root -m 0600", "install -o root -g root -m 0644", env_files_private),
    ("systemd/owlhead-executor.service", "User=owlhead_exec", "User=owlhead_api", executor_unit_hardened),
    ("systemd/owlhead-executor.service", "SocketBindDeny=any", "SocketBindAllow=tcp:9090\nSocketBindDeny=any", executor_unit_hardened),
    ("systemd/owlhead-executor.service", "NoNewPrivileges=yes", "NoNewPrivileges=no", executor_unit_hardened),
    ("systemd/owlhead-api.service", "UMask=0027", "StateDirectory=owlhead\nUMask=0027", api_unit_hardened),
    ("bootstrap.sh", '-m 0730 "$VAULT/tokens"', '-m 0770 "$VAULT/tokens"', vault_layout),
    ("bootstrap.sh", '-m 2770 "$VAULT/pending"', '-m 0770 "$VAULT/pending"', vault_layout),
    ("bootstrap.sh", '-o "$EXEC_USER" -g "$API_USER" -m 0730', '-o "$API_USER" -g "$API_USER" -m 0730', vault_layout),
    ("bootstrap.sh", "GRANT CONNECT ON DATABASE $DB TO $EXEC_USER;", "GRANT CONNECT ON DATABASE $DB TO $EXEC_USER;\nGRANT ALL ON SCHEMA journal TO $EXEC_USER;", api_role_minimal),
    ("executor.env.example", "ALPACA_OAUTH_CLIENT_ID=\n", "ALPACA_OAUTH_CLIENT_ID=PKAB12\n", example_names_only),
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
    ("api.env.example", "ALPACA_OAUTH_CLIENT_ID=\n", "ALPACA_OAUTH_CLIENT_ID=PKAB12\n", example_names_only),
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
