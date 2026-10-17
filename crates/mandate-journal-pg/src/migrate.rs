//! The journal schema's migration runner, which `mandate db migrate` runs as its own deploy step,
//! before any process starts and never at start (ADR-0001 ES-08, infrastructure OPS-13 and §7.2,
//! backlog E21-50, DEC-994). It is the one ledger of what is applied: it replaces the sqlx
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
use sqlx::PgPool;

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
const MIGRATIONS: [Migration; 1] = [Migration {
    version: 1,
    file: "0001_journal.sql",
    sql: include_str!("../../../migrations/0001_journal.sql"),
}];

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
    #[error("not implemented yet ({story})")]
    Unimplemented { story: &'static str },
}

impl MigrateError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::SchemaInvalid => "schema_invalid",
            Self::Unavailable(_) => "unavailable",
            Self::OwnerRoleNotHeld => "owner_role_not_held",
            Self::SchemaNotOwned => "schema_not_owned",
            Self::SchemaUnrecorded => "schema_unrecorded",
            Self::SchemaNewer { .. } => "schema_newer",
            Self::Changed { .. } => "migration_changed",
            Self::OutOfOrder { .. } => "migration_out_of_order",
            Self::Failed { .. } => "migration_failed",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}

impl From<sqlx::Error> for MigrateError {
    fn from(e: sqlx::Error) -> Self {
        Self::Unavailable(e)
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
    let _ = (known, applied);
    Err(MigrateError::Unimplemented { story: "E21-50" })
}

/// Applies [`migrations`] to `schema` in the database `pool` connects to, as described in the
/// module documentation. `pool`'s role must be able to create the two roles when they are missing,
/// create the schema, and `SET ROLE` [`OWNER_ROLE`]; a superuser can.
///
/// # Errors
/// As [`migrate_known`].
pub async fn migrate(pool: &PgPool, schema: &str) -> Result<MigrationReport, MigrateError> {
    migrate_known(pool, schema, migrations()).await
}

/// [`migrate`] over `known` in place of [`migrations`], so tests can run forward steps, a failing
/// step, and a downgrade with migrations of their own.
///
/// # Errors
/// [`MigrateError::SchemaInvalid`] before connecting; then, each having changed nothing,
/// [`MigrateError::OwnerRoleNotHeld`], [`MigrateError::SchemaNotOwned`],
/// [`MigrateError::SchemaUnrecorded`], a ledger refusal of [`applied_prefix`],
/// [`MigrateError::Failed`] for a migration whose SQL fails, or [`MigrateError::Unavailable`].
pub async fn migrate_known(
    pool: &PgPool,
    schema: &str,
    known: &[Migration],
) -> Result<MigrationReport, MigrateError> {
    let _ = (pool, schema, known, APP_ROLE);
    Err(MigrateError::Unimplemented { story: "E21-50" })
}
