"""A journal role added after bootstrap connects and reads once the migration Job has run (DEC-2083).

CloudNativePG runs roles.sql once, at bootstrap, and adds every later process's role as a managed
role: a LOGIN member of `mandate_journal_app`, with none of roles.sql's per-role grants. This test
replays that on a throwaway PostgreSQL 18 server: the journal database owned by the migrator,
roles.sql as the superuser, then a role added the managed way, which cannot connect. The workspace
chart's migrate.sh then runs as the migrator, as its pre-upgrade hook does, and the late role
connects with the chart's DSN options and reads the journal's events, while it still cannot write
outside what the group was granted.
"""

import os
import shutil
import socket
import subprocess
import tempfile
from collections.abc import Iterator
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ROLES_SQL = ROOT / "deploy" / "cell" / "charts" / "journal-db" / "files" / "roles.sql"
MIGRATE = ROOT / "deploy" / "cell" / "charts" / "workspace" / "files" / "migrate.sh"
MIGRATIONS = ROOT / "migrations"
# The search path the workspace chart's DSN sets in the connection's options (mandate.journalDsn).
OPTIONS = "-c search_path=journal"
LATE_ROLE = "mandate_late"


def postgres_bin() -> Path | None:
    for candidate in [os.environ.get("MANDATE_PG_BIN", ""), "/usr/lib/postgresql/18/bin"]:
        if candidate and (Path(candidate) / "initdb").is_file():
            return Path(candidate)
    found = shutil.which("initdb")
    return Path(found).parent if found else None


BIN = postgres_bin()
if BIN is None or shutil.which("psql") is None:
    pytest.skip("PostgreSQL 18's server binaries are not installed (.cursor/install.sh)", allow_module_level=True)


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@pytest.fixture(scope="module")
def server() -> Iterator[dict]:
    work = Path(tempfile.mkdtemp(prefix="journal-roles-"))
    data, sockets = work / "data", work / "sock"
    sockets.mkdir()
    run_as = {}
    if os.geteuid() == 0:
        shutil.chown(work, "postgres", "postgres")
        shutil.chown(sockets, "postgres", "postgres")
        run_as = {"user": "postgres"}
    subprocess.run(
        [BIN / "initdb", "-D", data, "-U", "postgres", "--auth=trust", "-E", "UTF8"],
        check=True, capture_output=True, **run_as,
    )
    port = free_port()
    subprocess.run(
        [BIN / "pg_ctl", "-D", data, "-w", "-l", work / "log", "-o", f"-k {sockets} -p {port} -c listen_addresses=''", "start"],
        check=True, capture_output=True, **run_as,
    )
    try:
        yield {"host": str(sockets), "port": port}
    finally:
        subprocess.run([BIN / "pg_ctl", "-D", data, "-m", "immediate", "stop"], capture_output=True, **run_as)
        shutil.rmtree(work, ignore_errors=True)


def psql(server: dict, user: str, sql: str, database: str = "mandate", options: str = "") -> subprocess.CompletedProcess:
    env = {**os.environ, "PGHOST": server["host"], "PGPORT": str(server["port"]), "PGOPTIONS": options}
    return subprocess.run(
        ["psql", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-tA", "-U", user, "-d", database, "-c", sql],
        capture_output=True, text=True, env=env,
    )


def ok(done: subprocess.CompletedProcess) -> str:
    assert done.returncode == 0, done.stderr
    return done.stdout.strip()


@pytest.fixture(scope="module")
def bootstrapped(server: dict) -> dict:
    """The journal as CloudNativePG bootstraps it, then a role added the managed way."""
    ok(psql(server, "postgres", "CREATE ROLE mandate_migrator LOGIN", database="postgres"))
    ok(psql(server, "postgres", "CREATE DATABASE mandate OWNER mandate_migrator", database="postgres"))
    env = {**os.environ, "PGHOST": server["host"], "PGPORT": str(server["port"])}
    roles = subprocess.run(
        ["psql", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "mandate", "-f", ROLES_SQL],
        capture_output=True, text=True, env=env,
    )
    assert roles.returncode == 0, roles.stderr
    ok(psql(server, "postgres", f"CREATE ROLE {LATE_ROLE} LOGIN INHERIT IN ROLE mandate_journal_app"))
    return server


def migrate(server: dict) -> subprocess.CompletedProcess:
    dsn = f"postgres://mandate_migrator@/mandate?host={server['host']}&port={server['port']}"
    env = {**os.environ, "MANDATE_JOURNAL_DSN": dsn, "MANDATE_MIGRATIONS": str(MIGRATIONS)}
    return subprocess.run(["bash", MIGRATE], capture_output=True, text=True, env=env)


def test_a_late_role_connects_and_reads_once_the_migration_job_has_run(bootstrapped: dict):
    before = psql(bootstrapped, LATE_ROLE, "SELECT 1", options=OPTIONS)
    assert before.returncode != 0 and "permission denied for database" in before.stderr, before.stderr
    first = migrate(bootstrapped)
    assert first.returncode == 0, first.stderr
    assert ok(psql(bootstrapped, LATE_ROLE, "SELECT count(*) FROM events", options=OPTIONS)) == "0"
    again = migrate(bootstrapped)
    assert again.returncode == 0 and "already applied" in again.stdout, again.stderr
    assert ok(psql(bootstrapped, LATE_ROLE, "SELECT count(*) FROM stream_heads", options=OPTIONS)) == "0"


def test_the_group_gains_nothing_a_role_did_not_already_have(bootstrapped: dict):
    assert migrate(bootstrapped).returncode == 0
    denied = [
        "CREATE TABLE journal.extra (x int)",
        "DELETE FROM events",
        "UPDATE events SET seq = seq",
        "CREATE TABLE public.extra (x int)",
    ]
    for statement in denied:
        done = psql(bootstrapped, LATE_ROLE, statement, options=OPTIONS)
        assert done.returncode != 0 and "permission denied" in done.stderr, (statement, done.stderr)
    other = psql(bootstrapped, LATE_ROLE, "SELECT 1", database="postgres", options=OPTIONS)
    assert other.returncode != 0, "the group connects to the journal's database alone"
