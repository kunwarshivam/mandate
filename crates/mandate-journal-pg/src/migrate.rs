//! The journal schema's migration runner, which `mandate db migrate` runs as its own deploy step,
//! before any process starts and never at start (ADR-0001 ES-08, infrastructure OPS-13 and §7.2,
//! backlog E21-50, DEC-1180). It is the one ledger of what is applied: it replaces the sqlx
//! embedded migrator, and takes over `deploy/bootstrap.sh`'s `deploy.migrations` rows the first
//! time it runs on a host that script bootstrapped.
//!
//! One run is one transaction under one advisory lock: it creates [`OWNER_ROLE`] and [`APP_ROLE`]
//! when they are missing, creates the schema owned by [`OWNER_ROLE`], and then, acting as
//! [`OWNER_ROLE`], applies each file of `migrations/` that its ledger does not hold, in order,
//! recording the file's SHA-256 beside its version. It refuses, having changed nothing, a ledger
//! that holds a migration this build does not know (a downgrade), a recorded file whose content
//! changed, a ledger that is not the known files' prefix, and a schema that holds objects but no
//! ledger.

use mandate_canon::Digest;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool, Postgres, Transaction, raw_sql};

use crate::{APP_ROLE, OWNER_ROLE};

/// One forward-only migration: its version, its file name in `migrations/`, and its SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub version: i64,
    pub file: &'static str,
    pub sql: &'static str,
}

impl Migration {
    /// The SHA-256 of the file's bytes, as 64 lowercase hex characters: what the ledger records and
    /// what `deploy/bootstrap.sh` recorded with `sha256sum`.
    pub fn sha256(&self) -> String {
        Digest::of(self.sql.as_bytes()).to_hex()
    }

    /// This migration as a ledger row records it.
    pub fn recorded(&self) -> Recorded {
        Recorded {
            version: self.version,
            file: self.file.to_owned(),
            sha256: self.sha256(),
        }
    }
}

/// `migrations/`, in order, embedded at build time. A new migration is a new file and a new entry,
/// never an edit.
const MIGRATIONS: [Migration; 2] = [
    Migration {
        version: 1,
        file: "0001_journal.sql",
        sql: include_str!("../../../migrations/0001_journal.sql"),
    },
    Migration {
        version: 2,
        file: "0002_news_reader_writes_only_news.sql",
        sql: include_str!("../../../migrations/0002_news_reader_writes_only_news.sql"),
    },
];

/// The journal schema's migrations, versions 1, 2, 3, … in order.
pub fn migrations() -> &'static [Migration] {
    &MIGRATIONS
}

/// The schema `deploy/bootstrap.sh` migrates, the only one whose `deploy.migrations` rows a first
/// run takes over.
pub const DEPLOY_SCHEMA: &str = "journal";

/// One row of a ledger: the version, the file name and the file's SHA-256 in lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub version: i64,
    pub file: String,
    pub sha256: String,
}

/// What one run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// How many `deploy.migrations` rows this run took over into its ledger.
    pub took_over: usize,
    /// The files the ledger already held, in order.
    pub already: Vec<&'static str>,
    /// The files this run applied, in order.
    pub applied: Vec<&'static str>,
    /// The schema's version after the run: the last applied migration's, 0 for none.
    pub version: i64,
}

/// Why a run applied nothing. Every refusal rolls the whole run back. No message names the DSN or
/// carries a driver error's text, which can name the host (`AGENTS.md` rule 7).
#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    /// The schema name is not a lowercase identifier of at most 63 bytes.
    #[error("the schema name is not a plain lowercase identifier")]
    SchemaInvalid,
    /// The database could not be reached, or a statement other than a migration's failed.
    #[error("the journal database is unavailable")]
    Unavailable(#[source] sqlx::Error),
    /// The connecting role lacks a privilege the run needs outside a migration's own SQL: to create
    /// a missing role or the schema, to read `deploy.migrations`, or to keep the ledger (SQLSTATE
    /// 42501). A migration's own denial is [`MigrateError::Failed`].
    #[error("the connecting role lacks a privilege the run needs (SQLSTATE 42501)")]
    PrivilegeMissing,
    /// The connecting role cannot act as [`OWNER_ROLE`].
    #[error("the connecting role cannot SET ROLE {OWNER_ROLE}")]
    OwnerRoleNotHeld,
    /// The schema exists and [`OWNER_ROLE`] does not own it.
    #[error("the schema is not owned by {OWNER_ROLE}")]
    SchemaNotOwned,
    /// The schema holds objects but no ledger records how they got there.
    #[error("the schema holds objects that no migration ledger records")]
    SchemaUnrecorded,
    /// The ledger holds a version this build does not know: a newer build migrated it.
    #[error("the schema is at version {version}, newer than this build knows; never rolled back")]
    SchemaNewer { version: i64 },
    /// A recorded migration's file name or SHA-256 differs from this build's.
    #[error("{file} was applied with other content; a migration is never edited")]
    Changed { file: String },
    /// The ledger is not exactly versions 1 to n: a version is missing or repeated.
    #[error("the ledger records version {version} ({file}) out of order")]
    OutOfOrder { version: i64, file: String },
    /// A migration's SQL failed; `sqlstate` is Postgres's error code.
    #[error("{file} failed with SQLSTATE {sqlstate}")]
    Failed {
        file: &'static str,
        sqlstate: String,
    },
}

impl MigrateError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::SchemaInvalid => "schema_invalid",
            Self::Unavailable(_) => "unavailable",
            Self::PrivilegeMissing => "privilege_missing",
            Self::OwnerRoleNotHeld => "owner_role_not_held",
            Self::SchemaNotOwned => "schema_not_owned",
            Self::SchemaUnrecorded => "schema_unrecorded",
            Self::SchemaNewer { .. } => "schema_newer",
            Self::Changed { .. } => "migration_changed",
            Self::OutOfOrder { .. } => "migration_out_of_order",
            Self::Failed { .. } => "migration_failed",
        }
    }
}

/// Postgres's `insufficient_privilege`.
const INSUFFICIENT_PRIVILEGE: &str = "42501";

impl From<sqlx::Error> for MigrateError {
    fn from(e: sqlx::Error) -> Self {
        let denied = e
            .as_database_error()
            .and_then(|d| d.code())
            .is_some_and(|code| code == INSUFFICIENT_PRIVILEGE);
        if denied {
            Self::PrivilegeMissing
        } else {
            Self::Unavailable(e)
        }
    }
}

/// How many of `known` the ledger rows `applied` hold, which must be exactly `known`'s first n,
/// in any order.
///
/// # Errors
/// [`MigrateError::SchemaNewer`] when any row's version is beyond `known`, checked first so a
/// downgrade is always named as one; then [`MigrateError::OutOfOrder`] for rows that are not
/// versions 1 to n each once, and [`MigrateError::Changed`] for a row whose file or SHA-256
/// differs from `known`'s at that version.
pub fn applied_prefix(known: &[Recorded], applied: &[Recorded]) -> Result<usize, MigrateError> {
    let last_known = i64::try_from(known.len()).unwrap_or(i64::MAX);
    let newest = applied
        .iter()
        .map(|r| r.version)
        .filter(|&v| v > last_known);
    if let Some(version) = newest.max() {
        return Err(MigrateError::SchemaNewer { version });
    }
    let mut by_version: Vec<&Recorded> = applied.iter().collect();
    by_version.sort_by_key(|r| r.version);
    for (expected, row) in (1i64..).zip(&by_version) {
        if row.version != expected {
            return Err(MigrateError::OutOfOrder {
                version: row.version,
                file: row.file.clone(),
            });
        }
    }
    for (row, known) in by_version.iter().zip(known) {
        if row.file != known.file || row.sha256 != known.sha256 {
            return Err(MigrateError::Changed {
                file: row.file.clone(),
            });
        }
    }
    Ok(by_version.len())
}

/// Applies [`migrations`] to `schema` in the database `pool` connects to, as described in the
/// module documentation. `pool`'s role must be able to create the two roles when they are missing,
/// create the schema, and `SET ROLE` [`OWNER_ROLE`]; a superuser can.
///
/// # Errors
/// As [`migrate_known`].
#[allow(
    clippy::disallowed_methods,
    reason = "the runner's own entry points call each other; the lint is for their callers"
)]
pub async fn migrate(pool: &PgPool, schema: &str) -> Result<MigrationReport, MigrateError> {
    migrate_known(pool, schema, migrations()).await
}

/// [`migrate`] over one connection to the database `dsn` names, closed before it returns. A DSN
/// that does not parse is [`MigrateError::Unavailable`], like one that does not connect, and
/// neither error carries any of its text.
///
/// # Errors
/// As [`migrate_known`].
#[allow(
    clippy::disallowed_methods,
    reason = "the runner's own entry points call each other; the lint is for their callers"
)]
pub async fn migrate_dsn(dsn: &str, schema: &str) -> Result<MigrationReport, MigrateError> {
    let options = dsn.parse::<PgConnectOptions>()?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy_with(options);
    let outcome = migrate(&pool, schema).await;
    pool.close().await;
    outcome
}

/// [`migrate`] over `known` in place of [`migrations`], so tests can run forward steps, a failing
/// step, and a downgrade with migrations of their own.
///
/// # Errors
/// [`MigrateError::SchemaInvalid`] before connecting; then, each having changed nothing,
/// [`MigrateError::PrivilegeMissing`], [`MigrateError::OwnerRoleNotHeld`],
/// [`MigrateError::SchemaNotOwned`], [`MigrateError::SchemaUnrecorded`], a ledger refusal of [`applied_prefix`],
/// [`MigrateError::Failed`] for a migration whose SQL fails, or [`MigrateError::Unavailable`].
pub async fn migrate_known(
    pool: &PgPool,
    schema: &str,
    known: &[Migration],
) -> Result<MigrationReport, MigrateError> {
    if !plain_identifier(schema) {
        return Err(MigrateError::SchemaInvalid);
    }
    let expected: Vec<Recorded> = known.iter().map(Migration::recorded).collect();
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(MIGRATION_LOCK)
        .execute(&mut *tx)
        .await?;
    raw_sql(CREATE_ROLES).execute(&mut *tx).await?;
    let held: bool = sqlx::query_scalar("SELECT pg_has_role(current_user, $1, 'SET')")
        .bind(OWNER_ROLE)
        .fetch_one(&mut *tx)
        .await?;
    if !held {
        return Err(MigrateError::OwnerRoleNotHeld);
    }
    let schema_owner: Option<String> = sqlx::query_scalar(
        "SELECT pg_get_userbyid(nspowner)::text FROM pg_namespace WHERE nspname = $1",
    )
    .bind(schema)
    .fetch_optional(&mut *tx)
    .await?;
    match schema_owner.as_deref() {
        None => {
            let create = format!("CREATE SCHEMA {schema} AUTHORIZATION {OWNER_ROLE}");
            raw_sql(AssertSqlSafe(create)).execute(&mut *tx).await?;
        }
        Some(OWNER_ROLE) => {}
        Some(_) => return Err(MigrateError::SchemaNotOwned),
    }
    let has_ledger = relation_exists(&mut tx, schema, LEDGER).await?;
    let deploy_rows = if !has_ledger && schema == DEPLOY_SCHEMA {
        deploy_ledger(&mut tx).await?
    } else {
        Vec::new()
    };
    let act_as_owner = format!(
        "SET LOCAL ROLE {OWNER_ROLE}; SET LOCAL search_path TO {schema}; \
         GRANT USAGE ON SCHEMA {schema} TO {APP_ROLE}"
    );
    raw_sql(AssertSqlSafe(act_as_owner))
        .execute(&mut *tx)
        .await?;
    let took_over = if has_ledger {
        0
    } else {
        let vouched = applied_prefix(&expected, &deploy_rows)?;
        if vouched == 0 && holds_objects(&mut tx, schema).await? {
            return Err(MigrateError::SchemaUnrecorded);
        }
        raw_sql(CREATE_LEDGER).execute(&mut *tx).await?;
        for row in &deploy_rows {
            record(&mut tx, row, DEPLOY_LEDGER).await?;
        }
        vouched
    };
    let ledger: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT version, file, sha256 FROM schema_migrations ORDER BY version")
            .fetch_all(&mut *tx)
            .await?;
    let ledger: Vec<Recorded> = ledger
        .into_iter()
        .map(|(version, file, sha256)| Recorded {
            version,
            file,
            sha256,
        })
        .collect();
    let held = applied_prefix(&expected, &ledger)?;
    let (already, pending) = known.split_at_checked(held).unwrap_or((known, &[]));
    for migration in pending {
        raw_sql(migration.sql)
            .execute(&mut *tx)
            .await
            .map_err(|e| failed(migration.file, e))?;
        record(&mut tx, &migration.recorded(), SOURCE).await?;
    }
    tx.commit().await?;
    Ok(MigrationReport {
        took_over,
        already: already.iter().map(|m| m.file).collect(),
        applied: pending.iter().map(|m| m.file).collect(),
        version: known.last().map_or(0, |m| m.version),
    })
}

/// The advisory lock every run holds for its whole transaction, so two runs never interleave.
const MIGRATION_LOCK: i64 = 0x4b30_4d49_4752;

/// The ledger's table, in the migrated schema, owned by [`OWNER_ROLE`] and readable by no one else.
const LEDGER: &str = "schema_migrations";

/// The `source` of a row this runner applied.
const SOURCE: &str = "mandate db migrate";

/// `deploy/bootstrap.sh`'s ledger, and the `source` of a row taken over from it.
const DEPLOY_LEDGER: &str = "deploy.migrations";

/// The two roles, `NOLOGIN`, when they are missing. A run in another database of the cluster may
/// create one at the same moment; either outcome leaves it existing.
const CREATE_ROLES: &str = "DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'mandate_journal_owner') THEN
        BEGIN
            CREATE ROLE mandate_journal_owner NOLOGIN;
        EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
        END;
    END IF;
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'mandate_journal_app') THEN
        BEGIN
            CREATE ROLE mandate_journal_app NOLOGIN;
        EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
        END;
    END IF;
END
$$";

const CREATE_LEDGER: &str = "CREATE TABLE schema_migrations (
    version bigint PRIMARY KEY CHECK (version >= 1),
    file text NOT NULL UNIQUE,
    sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    source text NOT NULL,
    applied_at timestamptz NOT NULL DEFAULT now()
);
REVOKE ALL ON schema_migrations FROM PUBLIC";

/// A lowercase SQL identifier that needs no quoting: a letter or `_`, then letters, digits and
/// `_`, at most 63 bytes (Postgres's `NAMEDATALEN - 1`).
fn plain_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    let first_ok = bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b == b'_');
    first_ok
        && name.len() <= 63
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

async fn relation_exists(
    tx: &mut Transaction<'static, Postgres>,
    schema: &str,
    relation: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2)",
    )
    .bind(schema)
    .bind(relation)
    .fetch_one(&mut **tx)
    .await
}

/// Whether `schema` holds any relation, function or type.
async fn holds_objects(
    tx: &mut Transaction<'static, Postgres>,
    schema: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                        WHERE n.nspname = $1) \
             OR EXISTS (SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                        WHERE n.nspname = $1) \
             OR EXISTS (SELECT 1 FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
                        WHERE n.nspname = $1)",
    )
    .bind(schema)
    .fetch_one(&mut **tx)
    .await
}

/// `deploy.migrations`'s rows, read as the connecting role, each with the version its file name
/// starts with (0 when it starts with none); empty when the table does not exist.
async fn deploy_ledger(
    tx: &mut Transaction<'static, Postgres>,
) -> Result<Vec<Recorded>, sqlx::Error> {
    if !relation_exists(tx, "deploy", "migrations").await? {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT file, sha256 FROM deploy.migrations ORDER BY file")
            .fetch_all(&mut **tx)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(file, sha256)| Recorded {
            version: file_version(&file),
            file,
            sha256,
        })
        .collect())
}

/// The version a `migrations/` file name starts with: its digits before the first `_`.
fn file_version(file: &str) -> i64 {
    file.split_once('_')
        .and_then(|(digits, _)| digits.parse().ok())
        .unwrap_or(0)
}

async fn record(
    tx: &mut Transaction<'static, Postgres>,
    row: &Recorded,
    source: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO schema_migrations (version, file, sha256, source) VALUES ($1, $2, $3, $4)",
    )
    .bind(row.version)
    .bind(&row.file)
    .bind(&row.sha256)
    .bind(source)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// A migration's SQL failed: Postgres's SQLSTATE names why. Any other failure is the database
/// being unavailable, and carries no text from it.
fn failed(file: &'static str, error: sqlx::Error) -> MigrateError {
    match &error {
        sqlx::Error::Database(e) => MigrateError::Failed {
            file,
            sqlstate: e.code().map(|c| c.into_owned()).unwrap_or_default(),
        },
        _ => MigrateError::Unavailable(error),
    }
}
