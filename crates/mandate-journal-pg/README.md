# mandate-journal-pg

The Postgres journal hot store ([journal spec](../../docs/specs/journal.md) §6.1, backlog E5-3,
DEC-109): the append protocol of `mandate-journal` in one transaction per append, with the
append-only rules enforced by the database itself. The schema is in `migrations/` at the repository
root.

## Running the tests

The tests need a Postgres superuser, because they create the two journal roles (once per cluster),
a schema per test, and plant faults as a superuser would. Without `MANDATE_PG_URL` each test says
it is skipping and passes; with `MANDATE_PG_REQUIRED` set as well (CI's `full` job), a missing
database fails instead.

On Ubuntu 24.04, with PostgreSQL 18 from the PGDG repository (`.cursor/install.sh` does this where
the repository is reachable):

```bash
sudo apt-get install -y postgresql-common
sudo /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y
sudo apt-get install -y postgresql-18
sudo pg_ctlcluster 18 main start
sudo -u postgres psql -c "ALTER USER postgres PASSWORD 'postgres'"

export MANDATE_PG_URL=postgres://postgres:postgres@localhost:5432/postgres
cargo nextest run -p mandate-journal-pg
cargo xtask ci postgres        # the same run CI's `full` job makes
```

With Docker instead: `docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=postgres postgres:18`.

Each test works in its own schema (`e5_3_<pid>_<n>`), dropped when the test ends, so tests run in
parallel against one database. The roles `mandate_journal_owner` and `mandate_journal_app` are
created `NOLOGIN` and left in place; the tests act as them with `SET ROLE`.

CI runs these tests against PostgreSQL 18 in `full` and against 17, the supported floor, nightly
(ADR-0001 ES-08).
