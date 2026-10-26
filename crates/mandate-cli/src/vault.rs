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
use std::iter;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use clap::{Args, Subcommand, ValueEnum};
use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::credentials::{
    check_credential_dir, read_token, read_token_file, write_credentials,
};
use mandate_vault::kube::kube_secrets;
use mandate_vault::kubernetes::{KubernetesAuth, kubernetes_mount_requests};
use mandate_vault::openbao::{
    OpenBaoConfig, OpenBaoSource, RoleCredentials, provision, provision_kubernetes_auth,
};
use mandate_vault::policy::{Identity, Mounts, PlatformSecret, SecretPath};
use mandate_vault::source::SecretSource;
use mandate_vault_local::VaultError;
use mandate_vault_local::import::{import_client_secret, resolve_ids};
use mandate_vault_local::startup::{Inputs, Role};
use mandate_vault_local::store::open;
use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretSlice, SecretString};

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
    /// Write a platform secret into an OpenBao vault as the secret-import identity. Stdin holds
    /// that identity's role id, its secret id, and the secret, one line each; none is ever an
    /// argument (DEC-2020, DEC-2630).
    Put(PutArgs),
}

#[derive(Debug, Args)]
pub struct PutArgs {
    /// Which platform secret stdin's last line is.
    #[arg(value_enum)]
    pub secret: PutSecret,
    /// The vault's address, `https://host:port`.
    #[arg(long)]
    pub address: String,
    /// The CA certificate (PEM) the vault's certificate must chain to.
    #[arg(long)]
    pub ca: PathBuf,
}

/// The platform secrets `vault put` writes (DEC-2020), and the platform's Alpaca OAuth client
/// secret (DEC-2636 item 1): each is written by the secret-import identity alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PutSecret {
    #[value(name = "model-provider-key")]
    ModelProviderKey,
    #[value(name = "email-credential")]
    EmailCredential,
    /// The Alpaca news reader's paper data key, one line `<key id>:<secret>` (DEC-2277).
    #[value(name = "news-data-key")]
    NewsDataKey,
    #[value(name = "alpaca-oauth-client-secret")]
    AlpacaOauthClientSecret,
}

/// Where `vault put` writes `secret`, and the name its one line of output gives it: a platform
/// secret at its own path, and the Alpaca OAuth client secret at [`SecretPath::ClientSecret`],
/// the one path the token exchange reads it from (DEC-2636 item 1).
pub fn put_path(secret: PutSecret) -> Result<(SecretPath, &'static str), SourceError> {
    Ok(match secret {
        PutSecret::ModelProviderKey => (
            SecretPath::Platform(PlatformSecret::ModelProviderKey),
            "model provider's key",
        ),
        PutSecret::EmailCredential => (
            SecretPath::Platform(PlatformSecret::EmailCredential),
            "email credential",
        ),
        PutSecret::NewsDataKey => (
            SecretPath::Platform(PlatformSecret::NewsDataKey),
            "news data key",
        ),
        PutSecret::AlpacaOauthClientSecret => {
            (SecretPath::ClientSecret, "Alpaca OAuth client secret")
        }
    })
}

/// The most bytes `vault put` reads from stdin: two credentials and a secret are far shorter.
pub const PUT_MAX: u64 = 16384;

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
        required_unless_present_any = ["secrets_namespace", "kubernetes_only"],
        conflicts_with = "secrets_namespace"
    )]
    pub out: Option<PathBuf>,
    /// Print the credentials instead, as Kubernetes Secrets in this namespace: one `v1` `List`
    /// on stdout, which must be a pipe to `kubectl apply -f -`, never a terminal; redirected to a
    /// file, it leaves every secret id there at your umask (DEC-2630).
    #[arg(long)]
    pub secrets_namespace: Option<String>,
    /// Also set Kubernetes auth up for the connections' processes, its TokenReview against the
    /// cluster's API server at `https://<IPv4>:<port>`, as the vault's pod reaches it (DEC-2060,
    /// DEC-2062). Required with `--secrets-namespace`, the cluster's path; `--out` may omit it.
    #[arg(long)]
    pub kubernetes_host: Option<String>,
    /// Set Kubernetes auth alone up, at `--kubernetes-host`, on a vault already provisioned: no
    /// AppRole role is touched and no secret id minted, so no credential in use is rotated, and
    /// nothing is printed. It takes no connection, `--out` or `--secrets-namespace`.
    #[arg(
        long,
        requires = "kubernetes_host",
        conflicts_with_all = ["out", "secrets_namespace", "connections"]
    )]
    pub kubernetes_only: bool,
}

/// The Kubernetes auth `--kubernetes-host` asks for: the API server at `host`, with no CA, so the
/// vault reads the one its own pod projects (DEC-2062 item 3), and `secrets_namespace`, the
/// workspace's, as the namespace the role provisioner's roles bind (DEC-2350 item 1). No host is no Kubernetes auth,
/// except with `--secrets-namespace`, the cluster's path, where a missing one is
/// [`SourceError::Config`], as is a host that is not `https://`, an IPv4 address and a port
/// ([`mandate_vault::kubernetes::api_server_address`]); both are refused before the vault is
/// reached.
pub fn kubernetes_auth(
    host: Option<&str>,
    secrets_namespace: Option<&str>,
) -> Result<Option<KubernetesAuth>, SourceError> {
    match (host, secrets_namespace) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(SourceError::Config),
        (Some(host), _) => {
            let auth = KubernetesAuth {
                host: host.to_owned(),
                ca_pem: None,
                namespace: secrets_namespace.map(str::to_owned),
            };
            kubernetes_mount_requests(&Mounts::default(), &auth)?;
            Ok(Some(auth))
        }
    }
}

/// Which of the process's standard streams are terminals, which `vault provision` refuses for its
/// token and its Secrets, and `vault put` for the credentials and the secret it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terminals {
    pub stdin: bool,
    pub stdout: bool,
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

/// Logs in as the secret-import identity with the role id and secret id on `stdin`'s first two
/// lines, and writes the third, the secret, at `args.secret`'s path (DEC-2020), printing only
/// which secret was written. Stdin is exactly three non-empty lines, the last newline optional,
/// with no carriage return, at most [`PUT_MAX`] bytes, the last newline counted among them;
/// anything else is refused `credential_file` before the vault is reached. A `stdin` that is a
/// terminal is refused `config` before it is read, so nothing is typed where it echoes. A refusal
/// names its reason code and carries no line of stdin. Its one line of output on success is shown
/// only by a real vault: the CLI refuses the dev server CI runs (DEC-977 item 6), so staging's
/// apply is where it is seen.
pub fn run_put(
    args: &PutArgs,
    stdin: &mut impl Read,
    terminals: Terminals,
    now: UtcNanos,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let refused = |e: SourceError| anyhow!("{e} ({})", e.code());
    if terminals.stdin {
        return Err(anyhow!(
            "the credentials and the secret are read only from a pipe, never a terminal (config)"
        ));
    }
    let mut bytes = Vec::new();
    let read = stdin
        .take(PUT_MAX.saturating_add(1))
        .read_to_end(&mut bytes);
    if read.is_err() || u64::try_from(bytes.len()).map_or(true, |read| read > PUT_MAX) {
        bytes.zeroize();
        return Err(refused(SourceError::CredentialFile));
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => SecretString::from(text),
        Err(invalid) => {
            invalid.into_bytes().zeroize();
            return Err(refused(SourceError::CredentialFile));
        }
    };
    let body = text.expose_secret();
    let lines: Vec<&str> = body
        .strip_suffix('\n')
        .unwrap_or(body)
        .split('\n')
        .collect();
    let [role_id, secret_id, value] = lines.as_slice() else {
        return Err(refused(SourceError::CredentialFile));
    };
    if [role_id, secret_id, value]
        .iter()
        .any(|line| line.is_empty() || line.contains('\r'))
    {
        return Err(refused(SourceError::CredentialFile));
    }
    let (path, name) = put_path(args.secret).map_err(refused)?;
    let config = OpenBaoConfig {
        address: args.address.clone(),
        ca_pem: fs::read(&args.ca).map_err(|_| refused(SourceError::Config))?,
        mounts: Mounts::default(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the put's runtime")?;
    runtime
        .block_on(async {
            let source = OpenBaoSource::login(
                &config,
                Identity::SecretImport,
                role_id,
                &SecretString::from((*secret_id).to_owned()),
                now,
            )
            .await?;
            source
                .put(&path, &SecretSlice::from(value.as_bytes().to_vec()))
                .await
        })
        .map_err(refused)?;
    writeln!(out, "the {name} is in the vault")?;
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
/// or the vault reached. Its output is meant for `kubectl apply -f -`: redirected to a file, it
/// leaves every secret id there at the caller's umask. With `--operator-token-stdin` the token is
/// read from `stdin` by [`mandate_vault::credentials::read_token`], and a `stdin` that is a
/// terminal is refused `config` before it is read, so a token is never typed where it echoes.
pub fn run_provision(
    args: &ProvisionArgs,
    stdin: &mut impl Read,
    terminals: Terminals,
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
    if let Some(namespace) = &args.secrets_namespace {
        if terminals.stdout {
            return Err(anyhow!(
                "the Secrets are printed only to a pipe, never a terminal (config)"
            ));
        }
        kube_secrets(namespace, &[]).map_err(refused)?;
    }
    if args.operator_token_stdin && terminals.stdin {
        return Err(anyhow!(
            "the operator's token is read only from a pipe, never a terminal (config)"
        ));
    }
    let operator = match &args.operator_token_file {
        Some(path) => read_token_file(path),
        None => read_token(stdin),
    }
    .map_err(refused)?;
    if let Some(dir) = &args.out {
        check_credential_dir(dir, &connections).map_err(refused)?;
    }
    let kubernetes = kubernetes_auth(
        args.kubernetes_host.as_deref(),
        args.secrets_namespace.as_deref(),
    )
    .map_err(refused)?;
    let config = OpenBaoConfig {
        address: args.address.clone(),
        ca_pem: fs::read(&args.ca).map_err(|_| refused(SourceError::Config))?,
        mounts: Mounts::default(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the provisioning's runtime")?;
    if args.kubernetes_only {
        let auth = kubernetes.ok_or_else(|| refused(SourceError::Config))?;
        return runtime
            .block_on(provision_kubernetes_auth(&config, &operator, &auth))
            .map_err(refused);
    }
    let credentials = runtime
        .block_on(provision(
            &config,
            &operator,
            &connections,
            kubernetes.as_ref(),
        ))
        .map_err(refused)?;
    match (&args.out, &args.secrets_namespace) {
        (Some(dir), _) => {
            for name in write_credentials(dir, &credentials).map_err(refused)? {
                writeln!(out, "{name}")?;
            }
        }
        (None, Some(namespace)) => write_secrets(namespace, &credentials, out)?,
        (None, None) => return Err(refused(SourceError::Config)),
    }
    Ok(())
}

/// Writes `credentials` to `out` as one `v1` `List` of their Secrets in `namespace`, on one line,
/// for `kubectl apply -f -`.
pub fn write_secrets(
    namespace: &str,
    credentials: &[RoleCredentials],
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let manifests =
        kube_secrets(namespace, credentials).map_err(|e| anyhow!("{e} ({})", e.code()))?;
    write!(out, "{{\"apiVersion\":\"v1\",\"kind\":\"List\",\"items\":[")?;
    let separators = iter::once("").chain(iter::repeat(","));
    for (separator, manifest) in separators.zip(&manifests) {
        write!(out, "{separator}{}", manifest.manifest.expose_secret())?;
    }
    writeln!(out, "]}}")?;
    Ok(())
}
