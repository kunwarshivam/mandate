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
import re
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
LATE_READER = "mandate_late_reader"
# roles.sql before the read-only group (DEC-2082), as a journal bootstrapped then holds it: the
# group's creation and grant removed, and its one member made the application group's.
OLD_ROLES_SQL = re.sub(
    r"  IF NOT EXISTS \(SELECT FROM pg_roles WHERE rolname = 'mandate_journal_reader'\) THEN\n"
    r"    CREATE ROLE mandate_journal_reader NOLOGIN;\n  END IF;\n",
    "",
    ROLES_SQL.read_text(),
).replace("IN ROLE mandate_journal_reader", "IN ROLE mandate_journal_app").replace(
    "GRANT USAGE ON SCHEMA journal TO mandate_journal_reader;\n", ""
)


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


def postgres() -> Iterator[dict]:
    """A throwaway PostgreSQL 18 server on a socket of its own, stopped and removed after."""
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


@pytest.fixture(scope="module")
def server() -> Iterator[dict]:
    yield from postgres()


def psql(server: dict, user: str, sql: str, database: str = "mandate", options: str = "") -> subprocess.CompletedProcess:
    env = {**os.environ, "PGHOST": server["host"], "PGPORT": str(server["port"]), "PGOPTIONS": options}
    return subprocess.run(
        ["psql", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-tA", "-U", user, "-d", database, "-c", sql],
        capture_output=True, text=True, env=env,
    )


def ok(done: subprocess.CompletedProcess) -> str:
    assert done.returncode == 0, done.stderr
    return done.stdout.strip()


def bootstrap(server: dict, roles_sql: str) -> None:
    """The journal as CloudNativePG bootstraps it: the migrator's database, then `roles_sql`."""
    ok(psql(server, "postgres", "CREATE ROLE mandate_migrator LOGIN", database="postgres"))
    ok(psql(server, "postgres", "CREATE DATABASE mandate OWNER mandate_migrator", database="postgres"))
    env = {**os.environ, "PGHOST": server["host"], "PGPORT": str(server["port"])}
    roles = subprocess.run(
        ["psql", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "mandate"],
        input=roles_sql, capture_output=True, text=True, env=env,
    )
    assert roles.returncode == 0, roles.stderr


@pytest.fixture(scope="module")
def bootstrapped(server: dict) -> dict:
    """The journal bootstrapped from today's roles.sql, then an application role and a read-only
    role added the managed way."""
    bootstrap(server, ROLES_SQL.read_text())
    ok(psql(server, "postgres", f"CREATE ROLE {LATE_ROLE} LOGIN INHERIT IN ROLE mandate_journal_app"))
    ok(psql(server, "postgres", f"CREATE ROLE {LATE_READER} LOGIN INHERIT IN ROLE mandate_journal_reader"))
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


def test_a_reader_selects_the_journal_and_writes_nothing(bootstrapped: dict):
    """DEC-2082: a member of the read-only group, as the data process is, reads the journal's
    tables once the Job has run, and every write is refused, since its binary refuses none."""
    assert migrate(bootstrapped).returncode == 0
    assert ok(psql(bootstrapped, LATE_READER, "SELECT count(*) FROM events", options=OPTIONS)) == "0"
    assert ok(psql(bootstrapped, LATE_READER, "SELECT count(*) FROM stream_heads", options=OPTIONS)) == "0"
    writes = [
        "INSERT INTO event_ids DEFAULT VALUES",
        "INSERT INTO events SELECT * FROM events",
        "UPDATE stream_heads SET seq = seq",
        "DELETE FROM events",
        "CREATE TABLE journal.extra (x int)",
    ]
    for statement in writes:
        done = psql(bootstrapped, LATE_READER, statement, options=OPTIONS)
        assert done.returncode != 0 and "permission denied" in done.stderr, (statement, done.stderr)


@pytest.fixture(scope="module")
def old_server() -> Iterator[dict]:
    yield from postgres()


def test_the_job_runs_on_a_journal_bootstrapped_before_the_reader_group(old_server: dict):
    """DEC-2082: on a journal whose bootstrap made no read-only group, the Job grants it nothing and
    still applies the migrations, so the workspace release never waits on the group; once the
    managed roles have made it, the next run grants it its reads."""
    bootstrap(old_server, OLD_ROLES_SQL)
    group = psql(old_server, "postgres", "SELECT count(*) FROM pg_roles WHERE rolname = 'mandate_journal_reader'")
    assert ok(group) == "0"
    done = migrate(old_server)
    assert done.returncode == 0 and "applied: 0001_journal.sql" in done.stdout, done.stderr
    ok(psql(old_server, "postgres", "CREATE ROLE mandate_journal_reader NOLOGIN"))
    ok(psql(old_server, "postgres", f"CREATE ROLE {LATE_READER} LOGIN INHERIT IN ROLE mandate_journal_reader"))
    denied = psql(old_server, LATE_READER, "SELECT 1", options=OPTIONS)
    assert denied.returncode != 0, "the group has nothing before the next run"
    again = migrate(old_server)
    assert again.returncode == 0, again.stderr
    assert ok(psql(old_server, LATE_READER, "SELECT count(*) FROM events", options=OPTIONS)) == "0"
