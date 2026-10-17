//! `mandate vault import alpaca-oauth-client-secret` (E10-13 V1, DEC-692 item 6): the platform's
//! client secret from stdin into the local vault, as the executor. The secret is never an
//! argument; the command prints nothing of it.
//!
//! It runs under `systemd-run` as `owlhead_exec` with both vault keys as credentials
//! (`deploy/set-secrets.sh`), so it reads `$CREDENTIALS_DIRECTORY`, `$MANDATE_VAULT_DIR`
//! (default [`DEFAULT_VAULT_DIR`]), and the service users' ids from `/etc/passwd` and
//! `/etc/group`.

use std::io::{Read, Write};

use anyhow::bail;
use clap::{Args, Subcommand, ValueEnum};

/// Where the demo host keeps the vault (DEC-692 item 1).
pub const DEFAULT_VAULT_DIR: &str = "/var/lib/owlhead/vault";

#[derive(Debug, Subcommand)]
pub enum VaultCommand {
    /// Read a platform secret from stdin, never from an argument, and store it in the local
    /// vault as the executor. Run it through `deploy/set-secrets.sh`.
    Import(ImportArgs),
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Which secret stdin holds.
    #[arg(value_enum)]
    pub secret: ImportedSecret,
}

/// The secrets `vault import` takes: only the Alpaca OAuth client secret (DEC-692 item 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ImportedSecret {
    #[value(name = "alpaca-oauth-client-secret")]
    AlpacaOauthClientSecret,
}

/// Runs the import with the process's `environment` and `stdin`, and reports to `out`.
pub fn run_import(
    args: &ImportArgs,
    environment: &[(String, String)],
    stdin: &mut impl Read,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let _ = (args, environment, stdin, out);
    bail!("E10-13 V1 has not been implemented yet")
}
