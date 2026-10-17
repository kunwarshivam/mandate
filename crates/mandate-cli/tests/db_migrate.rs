//! `mandate db migrate` (backlog E21-50, ADR-0001 ES-08, OPS-13, DEC-1180), run as the binary a
//! cell's migration Job runs: it creates the journal's roles and schema, applies `migrations/`
//! forward only under the owner role, prints what it did, takes over a host `deploy/bootstrap.sh`
//! migrated with each file applied once, refuses a downgrade or a changed file having changed
//! nothing, and never shows the DSN. Each Postgres test gets a database of its own, dropped at
//! the end, because the command migrates the fixed `journal` schema and reads `deploy.migrations`.

use std::process::{Command as Process, Output};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::Parser;
use clap::error::ErrorKind;
use mandate_canon::Digest;
use mandate_cli::db::{DbCommand, MigrateArgs, migrate};
use mandate_cli::{Cli, Command};
use mandate_journal_pg::{APP_ROLE, OWNER_ROLE, migrations};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool, raw_sql};
use tokio::runtime::Runtime;

const URL_VAR: &str = "MANDATE_PG_URL";
const REQUIRED_VAR: &str = "MANDATE_PG_REQUIRED";
const BOOTSTRAP: &str = include_str!("../../../deploy/bootstrap.sh");

const SENTINELS: [&str; 4] = [
    "k0-user-sentinel",
    "k0-password-sentinel",
    "k0-host-sentinel.invalid",
    "k0-db-sentinel",
];

#[test]
fn db_migrate_takes_a_journal_and_an_optional_schema() {
    let parsed = Cli::try_parse_from(["mandate", "db", "migrate", "--journal", "postgres://h/j"]);
    let Ok(Cli {
        command: Command::Db(DbCommand::Migrate(args)),
    }) = parsed
    else {
        panic!("{parsed:?}")
    };
    assert_eq!(
        args.schema, "journal",
        "the schema deploy/ and the cell use"
    );
    let named = [
        "mandate",
        "db",
        "migrate",
        "--journal",
        "j",
        "--schema",
        "s1",
    ];
    let Ok(Cli {
        command: Command::Db(DbCommand::Migrate(args)),
    }) = Cli::try_parse_from(named)
    else {
        panic!("--schema")
    };
    assert_eq!(args.schema, "s1");
    let kind = |argv: &[&str]| Cli::try_parse_from(argv).err().map(|e| e.kind());
    assert_eq!(
        kind(&["mandate", "db", "migrate"]),
        Some(ErrorKind::MissingRequiredArgument)
    );
    for extra in ["--down", "--force", "--to"] {
        assert_eq!(
            kind(&["mandate", "db", "migrate", "--journal", "j", extra]),
            Some(ErrorKind::UnknownArgument),
            "{extra}: forward only, nothing to force"
        );
    }
    let debug = format!(
        "{:?}",
        MigrateArgs {
            journal: SENTINELS[1].into(),
            schema: "journal".to_owned(),
        }
    );
    assert!(!debug.contains(SENTINELS[1]), "{debug}");
}

/// Runs the command in-process; it must refuse with `code`, print nothing, and name no part of
/// the DSN.
fn refuses(code: &str, args: &MigrateArgs) {
    let mut out = Vec::new();
    let why = migrate(args, &mut out).map(|r| format!("not refused: {r:?}"));
    let why = why.unwrap_or_else(|e| format!("{e:#}"));
    assert!(why.contains(code), "{code}: {why}");
    let named: Vec<&str> = SENTINELS.into_iter().filter(|s| why.contains(s)).collect();
    assert!(named.is_empty(), "{code} names {named:?}: {why}");
    assert!(
        out.is_empty(),
        "{code}: printed {}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn refusals_without_a_database_name_no_part_of_the_dsn() {
    let [user, password, host, db] = SENTINELS;
    let dsn = format!("postgres://{user}:{password}@{host}:1/{db}");
    let args = |schema: &str| MigrateArgs {
        journal: dsn.as_str().into(),
        schema: schema.to_owned(),
    };
    refuses("schema_invalid", &args("Journal"));
    refuses("schema_invalid", &args("j;drop"));
    refuses("unavailable", &args("journal"));
    let unparsable = MigrateArgs {
        journal: format!("{password}://{host}").as_str().into(),
        schema: "journal".to_owned(),
    };
    refuses("unavailable", &unparsable);
}

static DATABASES: AtomicU64 = AtomicU64::new(0);

/// A database of the test's own on the cluster `MANDATE_PG_URL` names, with a superuser pool in it.
/// The cluster-wide roles the command and `deploy/bootstrap.sh` create are made first, `NOLOGIN`,
/// tolerating a concurrent test making the same one, so neither races another test's.
struct Fresh {
    rt: Runtime,
    url: String,
    name: String,
    server: PgPool,
    pool: PgPool,
}

impl Fresh {
    fn new() -> Option<Self> {
        let Ok(url) = std::env::var(URL_VAR) else {
            assert!(
                std::env::var_os(REQUIRED_VAR).is_none(),
                "{REQUIRED_VAR} is set but {URL_VAR} is not"
            );
            eprintln!("skipping: {URL_VAR} is unset");
            return None;
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let name = format!(
            "k0db_{}_{}",
            std::process::id(),
            DATABASES.fetch_add(1, Ordering::Relaxed)
        );
        let base = PgConnectOptions::from_str(&url).unwrap();
        let (server, pool) = rt.block_on(async {
            let server = PgPoolOptions::new()
                .max_connections(1)
                .connect_with(base.clone())
                .await
                .unwrap_or_else(|e| panic!("cannot connect to {URL_VAR}: {e}"));
            let roles = [OWNER_ROLE, APP_ROLE, "owlhead_api", "owlhead_exec"].map(|role| {
                format!(
                    "DO $$ BEGIN CREATE ROLE {role} NOLOGIN; \
                     EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; END $$"
                )
            });
            let databases = [
                format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
                format!("CREATE DATABASE {name} TEMPLATE template0"),
            ];
            for sql in roles.into_iter().chain(databases) {
                raw_sql(AssertSqlSafe(sql)).execute(&server).await.unwrap();
            }
            let pool = PgPoolOptions::new()
                .max_connections(2)
                .connect_lazy_with(base.database(&name));
            (server, pool)
        });
        Some(Self {
            rt,
            url,
            name,
            server,
            pool,
        })
    }

    /// `MANDATE_PG_URL` with this database in place of its own.
    fn dsn(&self) -> String {
        let (scheme, rest) = self.url.split_once("://").unwrap();
        let (location, query) = match rest.split_once('?') {
            Some((l, q)) => (l, format!("?{q}")),
            None => (rest, String::new()),
        };
        let host = location.split_once('/').map_or(location, |(h, _)| h);
        format!("{scheme}://{host}/{}{query}", self.name)
    }

    fn sql(&self, sql: &str) {
        self.rt
            .block_on(raw_sql(AssertSqlSafe(sql.to_owned())).execute(&self.pool))
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    fn texts(&self, sql: &str) -> Vec<String> {
        self.rt
            .block_on(sqlx::query_scalar(AssertSqlSafe(sql.to_owned())).fetch_all(&self.pool))
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
    }

    fn ledger(&self, schema: &str) -> Vec<String> {
        self.texts(&format!(
            "SELECT version || ' ' || file || ' ' || sha256 || ' ' || source \
             FROM {schema}.schema_migrations ORDER BY version"
        ))
    }

    fn has_ledger(&self, schema: &str) -> bool {
        !self
            .texts(&format!(
                "SELECT relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = '{schema}' AND relname = 'schema_migrations'"
            ))
            .is_empty()
    }

    fn tables(&self, schema: &str) -> Vec<String> {
        self.texts(&format!(
            "SELECT tablename::text || ' ' || tableowner::text FROM pg_tables \
             WHERE schemaname = '{schema}' ORDER BY 1"
        ))
    }
}

impl Drop for Fresh {
    fn drop(&mut self) {
        let (pool, server) = (self.pool.clone(), self.server.clone());
        let drop_it = format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name);
        self.rt.block_on(async move {
            pool.close().await;
            raw_sql(AssertSqlSafe(drop_it)).execute(&server).await.ok();
            server.close().await;
        });
    }
}

/// The password and the host in `MANDATE_PG_URL`, which no output may show (`AGENTS.md` rule 7).
fn dsn_parts() -> Vec<String> {
    let url = std::env::var(URL_VAR).unwrap_or_default();
    let Some((credentials, location)) = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('@'))
    else {
        return Vec::new();
    };
    let password = credentials.split_once(':').map(|(_, p)| p.to_owned());
    let host = location.split(['/', ':', '?']).next().map(str::to_owned);
    [password, host]
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .collect()
}

/// Runs `mandate db migrate` as a process; neither of its outputs shows a part of the DSN.
fn db_migrate(dsn: &str, schema: Option<&str>) -> Output {
    let mut argv = vec!["db", "migrate", "--journal", dsn];
    if let Some(schema) = schema {
        argv.extend(["--schema", schema]);
    }
    let run = Process::new(env!("CARGO_BIN_EXE_mandate"))
        .args(&argv)
        .output()
        .unwrap();
    for part in dsn_parts() {
        for (name, out) in [("stdout", &run.stdout), ("stderr", &run.stderr)] {
            let shown = String::from_utf8_lossy(out);
            assert!(!shown.contains(&part), "{name} shows a DSN part: {shown}");
        }
    }
    run
}

fn succeeded(run: &Output) -> Vec<String> {
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{stderr}");
    String::from_utf8(run.stdout.clone())
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Refused with `code` on standard error, nothing on standard output.
fn refused(run: &Output, code: &str) {
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!run.status.success() && run.stdout.is_empty(), "{stderr}");
    assert!(stderr.contains(code), "{code}: {stderr}");
}

fn every_file_as(source: &str) -> Vec<String> {
    migrations()
        .iter()
        .map(|m| format!("{} {} {} {source}", m.version, m.file, m.sha256()))
        .collect()
}

fn version_line() -> String {
    format!("schema journal at version {}", migrations().len())
}

/// A fresh database: the roles, the schema and every file, each reported and recorded with its
/// SHA-256 once; a second run applies nothing and says so.
#[test]
fn the_binary_migrates_a_fresh_database_once() {
    let Some(db) = Fresh::new() else { return };
    let dsn = db.dsn();
    let first = succeeded(&db_migrate(&dsn, None));
    let mut expected: Vec<String> = migrations()
        .iter()
        .map(|m| format!("applied {}", m.file))
        .collect();
    expected.push(version_line());
    assert_eq!(first, expected);
    assert_eq!(db.ledger("journal"), every_file_as("mandate db migrate"));
    let owners = db.texts(&format!(
        "SELECT rolname::text FROM pg_roles WHERE rolname IN ('{OWNER_ROLE}', '{APP_ROLE}') \
         AND NOT rolcanlogin ORDER BY 1"
    ));
    assert_eq!(
        owners,
        vec![APP_ROLE, OWNER_ROLE],
        "both roles, neither can log in"
    );
    let schema_owner = db.texts(
        "SELECT pg_get_userbyid(nspowner)::text FROM pg_namespace WHERE nspname = 'journal'",
    );
    assert_eq!(schema_owner, vec![OWNER_ROLE]);
    assert!(
        db.tables("journal")
            .iter()
            .all(|t| t.ends_with(&format!(" {OWNER_ROLE}"))),
        "{:?}",
        db.tables("journal")
    );
    let second = succeeded(&db_migrate(&dsn, None));
    let mut expected: Vec<String> = migrations()
        .iter()
        .map(|m| format!("already applied {}", m.file))
        .collect();
    expected.push(version_line());
    assert_eq!(second, expected);
    assert_eq!(db.ledger("journal"), every_file_as("mandate db migrate"));
}

/// `deploy/bootstrap.sh`'s own database step, as it runs it: the heredoc between `sql "$DB"
/// <<SQL` and `SQL`, with its variables filled in. The two `REVOKE CONNECT` lines on `postgres`
/// and `template1` are left out, since they would change the shared test cluster and nothing here
/// depends on them.
fn bootstrap_schema_step(database: &str) -> String {
    let start = BOOTSTRAP.find("sql \"$DB\" <<SQL\n").unwrap() + "sql \"$DB\" <<SQL\n".len();
    let length = BOOTSTRAP[start..].find("\nSQL\n").unwrap();
    BOOTSTRAP[start..start + length]
        .lines()
        .filter(|l| !l.contains("DATABASE postgres") && !l.contains("DATABASE template1"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("\\$\\$", "$$")
        .replace("$API_USER", "owlhead_api")
        .replace("$EXEC_USER", "owlhead_exec")
        .replace("$DB", database)
}

/// `deploy/bootstrap.sh`'s migration loop for one file, as the script pipes it to psql.
fn bootstrap_applies(file: &str, sql: &str, digest: &str) -> String {
    assert!(
        BOOTSTRAP.contains(
            "printf 'BEGIN;\\nSET ROLE mandate_journal_owner;\\nSET search_path = journal;\\n'"
        ) && BOOTSTRAP.contains("INSERT INTO deploy.migrations (file, sha256) VALUES (%s, %s);"),
        "the bootstrap's migration step is what this test replays"
    );
    format!(
        "BEGIN;\nSET ROLE mandate_journal_owner;\nSET search_path = journal;\n{sql}\nRESET ROLE;\n\
         INSERT INTO deploy.migrations (file, sha256) VALUES ('{file}', '{digest}');\nCOMMIT;\n"
    )
}

/// A host `deploy/bootstrap.sh` migrated: the command takes its rows over, applies nothing again,
/// and from then on its own ledger is the record. Rows the command cannot vouch for are refused
/// with nothing created.
#[test]
fn the_binary_takes_over_a_host_deploy_bootstrapped() {
    let Some(db) = Fresh::new() else { return };
    db.sql(&bootstrap_schema_step(&db.name));
    for m in migrations() {
        db.sql(&bootstrap_applies(m.file, m.sql, &m.sha256()));
    }
    let tables = db.tables("journal");
    let lines = succeeded(&db_migrate(&db.dsn(), None));
    let mut expected = vec![format!(
        "took over {} files from deploy.migrations",
        migrations().len()
    )];
    expected.extend(
        migrations()
            .iter()
            .map(|m| format!("already applied {}", m.file)),
    );
    expected.push(version_line());
    assert_eq!(lines, expected);
    assert_eq!(db.ledger("journal"), every_file_as("deploy.migrations"));
    let mut with_ledger = tables.clone();
    with_ledger.push(format!("schema_migrations {OWNER_ROLE}"));
    with_ledger.sort();
    assert_eq!(db.tables("journal"), with_ledger, "each file applied once");
    let again = succeeded(&db_migrate(&db.dsn(), None));
    assert_eq!(
        again.first().map(String::as_str),
        migrations()
            .first()
            .map(|m| format!("already applied {}", m.file))
            .as_deref(),
        "the takeover happens once"
    );

    let Some(db) = Fresh::new() else { return };
    db.sql(&bootstrap_schema_step(&db.name));
    let first = migrations().first().unwrap();
    db.sql(&bootstrap_applies(
        first.file,
        first.sql,
        &Digest::of(b"x").to_hex(),
    ));
    refused(&db_migrate(&db.dsn(), None), "migration_changed");
    assert!(!db.has_ledger("journal"), "nothing was taken over");

    let Some(db) = Fresh::new() else { return };
    db.sql(&bootstrap_schema_step(&db.name));
    db.sql(&bootstrap_applies(first.file, first.sql, &first.sha256()));
    db.sql(&format!(
        "INSERT INTO deploy.migrations (file, sha256) VALUES ('9000_newer.sql', '{}')",
        "0".repeat(64)
    ));
    refused(&db_migrate(&db.dsn(), None), "schema_newer");
    assert!(!db.has_ledger("journal"));

    let other = succeeded(&db_migrate(&db.dsn(), Some("other")));
    assert!(
        other.iter().all(|l| !l.starts_with("took over")),
        "deploy.migrations describes the journal schema only: {other:?}"
    );
    assert_eq!(db.ledger("other"), every_file_as("mandate db migrate"));
}

/// A schema with objects and no ledger, and a ledger from a newer build, are refused with nothing
/// changed.
#[test]
fn the_binary_refuses_what_it_does_not_know() {
    let Some(db) = Fresh::new() else { return };
    db.sql(&format!(
        "CREATE SCHEMA journal AUTHORIZATION {OWNER_ROLE}; CREATE TABLE journal.events (x int)"
    ));
    refused(&db_migrate(&db.dsn(), None), "schema_unrecorded");
    assert!(!db.has_ledger("journal"));
    db.sql("DROP SCHEMA journal CASCADE");
    succeeded(&db_migrate(&db.dsn(), None));
    db.sql(&format!(
        "INSERT INTO journal.schema_migrations (version, file, sha256, source) \
         VALUES (9000, '9000_newer.sql', '{}', 'a newer build')",
        "0".repeat(64)
    ));
    refused(&db_migrate(&db.dsn(), None), "schema_newer");
    assert_eq!(db.ledger("journal").len(), migrations().len() + 1);
}
