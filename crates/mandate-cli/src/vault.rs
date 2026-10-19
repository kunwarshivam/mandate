//! `mandate vault import alpaca-oauth-client-secret` (E10-13 V1, DEC-692 item 6): the platform's
//! client secret from stdin into the local vault, as the executor. The secret is never an
//! argument; the command prints nothing of it.
//!
//! `mandate vault provision` (E21-32, DEC-2022 item 2) sets an OpenBao vault up for the cell's
//! connections and writes each process's AppRole credentials as owner-only files. The operator's
//! token is read from a file only its owner can read, never from an argument or the environment.
//!
//! It runs under `systemd-run` as `owlhead_exec` with both vault keys as credentials
//! (`deploy/set-secrets.sh`), so it reads `$CREDENTIALS_DIRECTORY`, `$MANDATE_VAULT_DIR`
//! (default [`DEFAULT_VAULT_DIR`]), and the service users' ids from `/etc/passwd` and
//! `/etc/group` ([`UserDatabase::from_host`]).

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use clap::{Args, Subcommand, ValueEnum};
use mandate_connections::record::ConnectionId;
use mandate_vault::SourceError;
use mandate_vault::credentials::{check_credential_dir, read_token_file, write_credentials};
use mandate_vault::openbao::{OpenBaoConfig, provision};
use mandate_vault::policy::Mounts;
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
    /// Set an OpenBao vault up for the cell's connections and write each process's AppRole
    /// credentials, owner-only, into `--out`. The operator's token is read from a file.
    Provision(ProvisionArgs),
}

#[derive(Debug, Args)]
pub struct ProvisionArgs {
    /// The vault's address, `https://host:port`.
    #[arg(long)]
    pub address: String,
    /// The CA certificate (PEM) the vault's certificate must chain to.
    #[arg(long)]
    pub ca: PathBuf,
    /// A file holding the operator's token, readable by its owner alone.
    #[arg(
        long,
        required_unless_present = "operator_token_stdin",
        conflicts_with = "operator_token_stdin"
    )]
    pub operator_token_file: Option<PathBuf>,
    /// Read the operator's token from stdin, a pipe its caller owns, instead (DEC-2630).
    #[arg(long)]
    pub operator_token_stdin: bool,
    /// A connection the vault holds credentials for; repeat it for each.
    #[arg(long = "connection")]
    pub connections: Vec<String>,
    /// The directory the credentials are written to, made with mode 0700 when absent.
    #[arg(
        long,
        required_unless_present = "secrets_namespace",
        conflicts_with = "secrets_namespace"
    )]
    pub out: Option<PathBuf>,
    /// Print the credentials instead, as Kubernetes Secrets in this namespace: one `v1` `List`
    /// on stdout, which must be a pipe, never a terminal (DEC-2630).
    #[arg(long)]
    pub secrets_namespace: Option<String>,
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

/// Runs the provisioning and prints, to `out`, the name of each file written and nothing else.
/// `--out` is checked before the vault is reached, so a directory the write would refuse leaves no
/// secret id issued and unwritten. A refusal names its reason code; none carries a token, a
/// secret id, or a response body.
///
/// With `--secrets-namespace` it writes no file and prints instead the credentials' Secrets as one
/// `v1` `List` (DEC-2630), for its caller to hand `kubectl` on stdin. A namespace that is not a
/// Kubernetes label, or an `out` that is a terminal, is refused `config` before the token is read
/// or the vault reached. With `--operator-token-stdin` the token is read from `stdin` by
/// [`mandate_vault::credentials::read_token`].
pub fn run_provision(
    args: &ProvisionArgs,
    stdin: &mut impl Read,
    stdout_is_terminal: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let refused = |e: SourceError| anyhow!("{e} ({})", e.code());
    let connections = args
        .connections
        .iter()
        .map(|id| {
            ConnectionId::new(id).map_err(|_| anyhow!("a connection id is malformed (connection)"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let _ = (stdin, stdout_is_terminal);
    let (Some(token_file), Some(dir)) = (&args.operator_token_file, &args.out) else {
        return Err(anyhow!(
            "{:?}",
            SourceError::Unimplemented { story: "E21-32" }
        ));
    };
    let operator = read_token_file(token_file).map_err(refused)?;
    check_credential_dir(dir, &connections).map_err(refused)?;
    let config = OpenBaoConfig {
        address: args.address.clone(),
        ca_pem: fs::read(&args.ca).map_err(|_| refused(SourceError::Config))?,
        mounts: Mounts::default(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the provisioning's runtime")?;
    let credentials = runtime
        .block_on(provision(&config, &operator, &connections))
        .map_err(refused)?;
    for name in write_credentials(dir, &credentials).map_err(refused)? {
        writeln!(out, "{name}")?;
    }
    Ok(())
}
