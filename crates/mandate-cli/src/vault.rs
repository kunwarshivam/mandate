//! `mandate vault import alpaca-oauth-client-secret` (E10-13 V1, DEC-692 item 6): the platform's
//! client secret from stdin into the local vault, as the executor. The secret is never an
//! argument; the command prints nothing of it.
//!
//! It runs under `systemd-run` as `owlhead_exec` with both vault keys as credentials
//! (`deploy/set-secrets.sh`), so it reads `$CREDENTIALS_DIRECTORY`, `$MANDATE_VAULT_DIR`
//! (default [`DEFAULT_VAULT_DIR`]), and the service users' ids from `/etc/passwd` and
//! `/etc/group` ([`UserDatabase::from_host`]).

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, anyhow};
use clap::{Args, Subcommand, ValueEnum};
use mandate_vault_local::VaultError;
use mandate_vault_local::import::{import_client_secret, resolve_ids};
use mandate_vault_local::startup::{Inputs, Role};
use mandate_vault_local::store::open;

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

/// The host's user and group databases, as the text of `/etc/passwd` and `/etc/group`.
#[derive(Debug, Clone)]
pub struct UserDatabase {
    pub passwd: String,
    pub group: String,
}

impl UserDatabase {
    /// Reads `/etc/passwd` and `/etc/group`.
    pub fn from_host() -> anyhow::Result<Self> {
        Ok(Self {
            passwd: fs::read_to_string("/etc/passwd").context("reading /etc/passwd")?,
            group: fs::read_to_string("/etc/group").context("reading /etc/group")?,
        })
    }
}

/// Runs the import with the process's `environment`, the host's `users`, and `stdin`, and
/// reports to `out`.
pub fn run_import(
    args: &ImportArgs,
    environment: &[(String, String)],
    users: &UserDatabase,
    stdin: &mut impl Read,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let ImportedSecret::AlpacaOauthClientSecret = args.secret;
    let variable = |name: &str| {
        environment
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let credentials_dir = variable("CREDENTIALS_DIRECTORY").ok_or_else(|| {
        anyhow!(
            "CREDENTIALS_DIRECTORY is not set: run the import through deploy/set-secrets.sh, \
             which loads both vault keys as systemd credentials"
        )
    })?;
    let vault_dir = variable("MANDATE_VAULT_DIR").unwrap_or(DEFAULT_VAULT_DIR);
    let ids =
        resolve_ids(&users.passwd, &users.group).map_err(|e| anyhow!("{e} ({})", e.code()))?;
    let inputs = Inputs {
        role: Role::Executor,
        vault_dir: Path::new(vault_dir),
        credentials_dir: Path::new(credentials_dir),
        environment,
        ids,
    };
    let refused = |e: VaultError| anyhow!("{e} ({})", e.code());
    let vault = open(&inputs)
        .and_then(|v| v.into_executor())
        .map_err(refused)?;
    import_client_secret(&vault, stdin).map_err(refused)?;
    writeln!(out, "the Alpaca OAuth client secret is in the vault")?;
    Ok(())
}
