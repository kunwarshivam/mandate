//! `mandate db migrate` (backlog E21-50, ADR-0001 ES-08, infrastructure OPS-13 and §7.2,
//! DEC-994): the deploy step that creates the journal's roles and schema and applies
//! `migrations/` forward only under the migration-owner role, before any process starts. A cell
//! runs it as its migration Job; no process migrates at start, which the workspace's
//! `disallowed-methods` lint enforces. Its whole interface is `--journal` and `--schema`, its
//! report on standard output, and its exit status.

use std::io::Write;

use anyhow::Context;
use clap::{Args, Subcommand};
use mandate_journal_pg::{MigrationReport, migrate_dsn};
use secrecy::{ExposeSecret, SecretString};

#[derive(Debug, Subcommand)]
pub enum DbCommand {
    /// Create the journal's roles and schema, and apply every migration not yet applied, in
    /// order, under the migration-owner role. Refuses a downgrade, a changed migration, and a
    /// schema it does not know, having changed nothing.
    Migrate(MigrateArgs),
}

#[derive(Debug, Args)]
pub struct MigrateArgs {
    /// The journal's Postgres DSN, as a role that can create roles and schemas and SET ROLE
    /// mandate_journal_owner. Prefer one without a password (set PGPASSWORD or PGPASSFILE);
    /// nothing prints it either way.
    #[arg(long, value_name = "DSN")]
    pub journal: SecretString,
    /// The schema to migrate.
    #[arg(long, value_name = "NAME", default_value = mandate_journal_pg::migrate::DEPLOY_SCHEMA)]
    pub schema: String,
}

/// Runs the migration and prints what it did: one line per file, already applied or applied
/// now, a line for any `deploy.migrations` rows it took over, then the schema's version.
///
/// # Errors
/// A refusal whose message carries the runner's code (`schema_invalid`, `unavailable`,
/// `owner_role_not_held`, `schema_not_owned`, `schema_unrecorded`, `schema_newer`,
/// `migration_changed`, `migration_out_of_order`, `migration_failed`), having printed nothing; no
/// message names the DSN.
#[allow(
    clippy::disallowed_methods,
    reason = "this command is the one place the journal schema is migrated (ES-08, OPS-13)"
)]
pub fn migrate(args: &MigrateArgs, report: &mut impl Write) -> anyhow::Result<MigrationReport> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the migration's runtime")?;
    let outcome = runtime.block_on(migrate_dsn(args.journal.expose_secret(), &args.schema));
    let done = outcome.map_err(|e| refused(e.code(), &e.to_string()))?;
    if done.took_over > 0 {
        writeln!(
            report,
            "took over {} files from deploy.migrations",
            done.took_over
        )?;
    }
    for file in &done.already {
        writeln!(report, "already applied {file}")?;
    }
    for file in &done.applied {
        writeln!(report, "applied {file}")?;
    }
    writeln!(report, "schema {} at version {}", args.schema, done.version)?;
    Ok(done)
}

/// A refusal: its stable code, then the runner's message, which names no part of the DSN.
fn refused(code: &str, message: &str) -> anyhow::Error {
    anyhow::anyhow!("db migrate refused: {code}: {message}")
}
