//! `cargo xtask gitops`: the founder's one bootstrap of a hosted cluster, and its workload secrets
//! (E21-58, DEC-2015, DEC-2018).
//!
//! The usual path is CI: lane IAC's `cargo xtask infra apply` calls [`bootstrap_from_infra`] in
//! process, from `infra.yml` on `main`, with the runner's own address opened on the control-plane
//! firewall for the run (DEC-2018 item 7). The commands below are the founder's break-glass path:
//! the firewall otherwise admits only the founder's addresses (`KUBE_API_ALLOWED_CIDRS`), so they
//! run on the founder's machine, with the founder's kubeconfig, and nowhere else. They refuse to run in CI or without a terminal, and they apply
//! nothing until the founder types the kubeconfig's current context, so an agent never applies
//! anything to a real cluster (DEC-2015 item 5). The workload secrets come from the founder's
//! shell: Barman's R2 token, which the founder makes by hand, and lane IAC's `tunnel_token` output
//! (deploy/gitops/BOOTSTRAP.md). Each reaches only `kubectl`'s stdin and is never printed.
//!
//! `gitops bootstrap <environment>` installs Flux's three controllers at the pinned version, each
//! image pinned by digest, writes the workload secrets, and applies the environment's `sync.yaml`.
//! From then on, Flux reconciles `deploy/gitops/clusters/<environment>` from `main`.
//! `gitops secrets <environment>` writes the workload secrets alone, to set or rotate them.

use std::env;
use std::fs;
use std::io::{self, BufRead as _, IsTerminal as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};

use crate::image;

/// The hosted clusters, one a directory under `deploy/gitops/clusters/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Environment {
    Staging,
    Production,
}

impl Environment {
    pub(crate) fn parse(name: &str) -> Result<Self> {
        match name {
            "staging" => Ok(Self::Staging),
            "production" => Ok(Self::Production),
            other => bail!("no hosted environment {other:?}: staging or production"),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Staging => "staging",
            Self::Production => "production",
        }
    }
}

/// The Flux components a hosted cluster runs, and nothing else (DEC-2015 item 1).
const FLUX_COMPONENTS: &str = "source-controller,kustomize-controller,helm-controller";

/// Each image `flux install --export` names for v2.9.6, with the digest it is pinned to
/// (`docs/dependencies.md`, Flux controller images).
const FLUX_IMAGES: [(&str, &str); 3] = [
    (
        "ghcr.io/fluxcd/source-controller:v1.9.6",
        "sha256:6a6693172589f8ff26123a231d5fa6ceb194a6efb4dc647cdf057c959f76a2e3",
    ),
    (
        "ghcr.io/fluxcd/kustomize-controller:v1.9.6",
        "sha256:2ebeaa341da77d52b6abbbba5efcee0450d47f8b42f0e6f33b08f9020262d606",
    ),
    (
        "ghcr.io/fluxcd/helm-controller:v1.6.5",
        "sha256:0d52fff5c4d476277b8fcb6beb9041e269adb5db943fe69f5a806ea0c92b1511",
    ),
];

/// The namespace every workload secret lives in: the one workspace's (infrastructure §3.2).
const WORKSPACE_NAMESPACE: &str = "ws-ws1";

/// A Secret the charts name but the cluster cannot make itself: its name and, for each of its
/// keys, the environment secret that holds the value (DEC-2015 item 4).
struct WorkloadSecret {
    name: &'static str,
    keys: &'static [(&'static str, &'static str)],
}

/// The archive's access key, which `journal-db`'s `archive.keyFrom` names, and the tunnel's token,
/// which the workspace chart's `ingress.cloudflared.tokenFrom` names.
const WORKLOAD_SECRETS: [WorkloadSecret; 2] = [
    WorkloadSecret {
        name: "journal-archive-key",
        keys: &[
            ("ACCESS_KEY_ID", "JOURNAL_ARCHIVE_ACCESS_KEY_ID"),
            ("ACCESS_SECRET_KEY", "JOURNAL_ARCHIVE_SECRET_KEY"),
        ],
    },
    WorkloadSecret {
        name: "cloudflared-tunnel",
        keys: &[("token", "CLOUDFLARED_TUNNEL_TOKEN")],
    },
];

/// One secret value, held in memory only. In GitHub Actions, constructing it masks the value in the
/// run's log at once, in every form a later line could print it ([`mask_forms`]): the repository
/// is public, so its logs are, and GitHub masks only `secrets.*` values on its own. `Debug` hides
/// it.
pub(crate) struct Secret(String);

impl Secret {
    /// The value, masked first when this runs in GitHub Actions.
    pub(crate) fn new(value: String) -> Self {
        if env::var("GITHUB_ACTIONS").is_ok_and(|actions| actions == "true") {
            let mut out = io::stdout().lock();
            for form in mask_forms(&value) {
                let _ = writeln!(out, "::add-mask::{form}");
            }
            let _ = out.flush();
        }
        Self(value)
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Every form of `value` a log line could carry, each one line and non-empty: the value, its
/// base64, base64url (unpadded), URL-encoded and JSON-escaped forms, and each of its lines.
fn mask_forms(value: &str) -> Vec<String> {
    let base64 = image::base64(value.as_bytes());
    let base64url = base64
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_owned();
    let mut url = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            url.push(char::from(byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    let json = serde_json::Value::String(value.to_owned()).to_string();
    let json = json
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(&json)
        .to_owned();
    let mut forms = vec![value.to_owned(), base64, base64url, url, json];
    forms.extend(value.lines().map(str::to_owned));
    let mut kept: Vec<String> = Vec::new();
    for form in forms {
        let form = form.trim().to_owned();
        if !form.is_empty() && !form.contains(['\n', '\r']) && !kept.contains(&form) {
            kept.push(form);
        }
    }
    kept
}

/// The workload secrets' values, held in memory only and never printed.
#[derive(Debug)]
pub(crate) struct WorkloadSecrets {
    /// The archive's R2 access key id: the bucket-scoped token's id.
    pub(crate) archive_access_key_id: Secret,
    /// The archive's R2 secret: the SHA-256 of that token's value.
    pub(crate) archive_secret_access_key: Secret,
    /// The tunnel's token.
    pub(crate) tunnel_token: Secret,
}

impl WorkloadSecrets {
    /// The founder's terminal: each value from the shell variable [`WORKLOAD_SECRETS`] names.
    fn from_env() -> Result<Self> {
        let read =
            |var: &str| env::var(var).with_context(|| format!("{var} is not set (BOOTSTRAP.md)"));
        Ok(Self {
            archive_access_key_id: Secret::new(read("JOURNAL_ARCHIVE_ACCESS_KEY_ID")?),
            archive_secret_access_key: Secret::new(read("JOURNAL_ARCHIVE_SECRET_KEY")?),
            tunnel_token: Secret::new(read("CLOUDFLARED_TUNNEL_TOKEN")?),
        })
    }

    /// Each Secret's manifest, in [`WORKLOAD_SECRETS`]' order.
    fn manifests(&self) -> Result<Vec<(&'static str, String)>> {
        let mut manifests = Vec::new();
        for secret in WORKLOAD_SECRETS {
            let mut values = Vec::new();
            for (key, var) in secret.keys {
                let value = match *var {
                    "JOURNAL_ARCHIVE_ACCESS_KEY_ID" => &self.archive_access_key_id,
                    "JOURNAL_ARCHIVE_SECRET_KEY" => &self.archive_secret_access_key,
                    "CLOUDFLARED_TUNNEL_TOKEN" => &self.tunnel_token,
                    other => bail!("no workload secret holds {other}"),
                };
                values.push((*key, value.expose().to_owned()));
            }
            manifests.push((secret.name, secret_manifest(secret.name, &values)?));
        }
        Ok(manifests)
    }
}

/// The cluster a run applies to: a kubeconfig file, when not the default one, and the context in it.
struct Cluster<'a> {
    kubeconfig: Option<&'a Path>,
    context: String,
}

/// The workflow that alone may bootstrap from CI: lane IAC's `infra.yml` on `main` (DEC-2018).
const INFRA_WORKFLOW_SUFFIX: &str = "/.github/workflows/infra.yml@refs/heads/main";

/// Why a CI run must not bootstrap, if it must not: only GitHub Actions, on `main`, in
/// `infra.yml`, may (DEC-2018 item 7).
fn infra_refusal(actions: &str, reference: &str, workflow: &str) -> Option<&'static str> {
    if actions != "true" {
        Some("the CI bootstrap runs only in GitHub Actions")
    } else if reference != "refs/heads/main" {
        Some("the CI bootstrap runs only on main")
    } else if !workflow.ends_with(INFRA_WORKFLOW_SUFFIX) {
        Some("the CI bootstrap runs only from infra.yml")
    } else {
        None
    }
}

/// The `flux` the pins in [`FLUX_IMAGES`] belong to.
const FLUX_VERSION: &str = "2.9.6";

/// `manifest`, from `flux install --export`, with every image pinned by digest. It refuses an image
/// that [`FLUX_IMAGES`] does not name, and a pinned image the manifest never uses, since either
/// means the CLI is not the pinned version.
fn pin_images(manifest: &str) -> Result<String> {
    let mut used = [false; FLUX_IMAGES.len()];
    let mut out = String::with_capacity(manifest.len());
    for line in manifest.lines() {
        let trimmed = line.trim_start();
        let item = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        if let Some(image) = item.strip_prefix("image: ") {
            let index = FLUX_IMAGES
                .iter()
                .position(|(tag, _)| *tag == image.trim())
                .with_context(|| format!("flux install names an image not pinned here: {image}"))?;
            let (tag, digest) = FLUX_IMAGES[index];
            used[index] = true;
            let indent = &line[..line.len() - item.len()];
            out.push_str(&format!("{indent}image: {tag}@{digest}\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if let Some(index) = used.iter().position(|used| !used) {
        bail!(
            "flux install does not use {}: not the pinned flux",
            FLUX_IMAGES[index].0
        );
    }
    Ok(out)
}

/// The Secret `name` in the workspace namespace, holding `values` base64-encoded, as JSON for
/// `kubectl apply`. An empty value is refused, since it would leave the chart a Secret that
/// cannot work.
fn secret_manifest(name: &str, values: &[(&str, String)]) -> Result<String> {
    let mut data = serde_json::Map::new();
    for (key, value) in values {
        ensure!(!value.is_empty(), "the Secret {name}'s {key} is empty");
        data.insert(
            (*key).to_owned(),
            serde_json::Value::String(image::base64(value.as_bytes())),
        );
    }
    let secret = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {
            "name": name,
            "namespace": WORKSPACE_NAMESPACE,
            "labels": {"app.kubernetes.io/managed-by": "mandate-gitops-bootstrap"},
        },
        "type": "Opaque",
        "data": data,
    });
    Ok(secret.to_string())
}

/// Why a run in this environment must not touch a cluster, if it must not: in CI, or with no
/// terminal to confirm on, as an agent's shell has none (DEC-2015 item 5).
fn refusal(ci: bool, terminal: bool) -> Option<&'static str> {
    if ci {
        Some(
            "`cargo xtask gitops` never runs in CI: the cluster's API admits only the founder's addresses",
        )
    } else if !terminal {
        Some("`cargo xtask gitops` runs only at the founder's terminal, which confirms the cluster")
    } else {
        None
    }
}

/// Whether `typed`, the founder's answer, names `context` exactly.
fn confirmed(typed: &str, context: &str) -> bool {
    !context.is_empty() && typed.trim() == context
}

/// Refuses unless the founder, at a terminal outside CI, types the kubeconfig's current context,
/// which this prints with the environment it will change. Returns that context.
fn founder_confirms(environment: Environment, action: &str) -> Result<String> {
    let ci = env::var_os("CI").is_some() || env::var_os("GITHUB_ACTIONS").is_some();
    confirm(
        ci,
        io::stdin().is_terminal(),
        environment,
        action,
        current_context,
        || {
            let mut typed = String::new();
            io::stdin()
                .lock()
                .read_line(&mut typed)
                .context("reading the answer")?;
            Ok(typed)
        },
    )
}

/// The kubeconfig's current context.
fn current_context() -> Result<String> {
    let out = Command::new("kubectl")
        .args(["config", "current-context"])
        .output()
        .context("starting kubectl (deploy/gitops/BOOTSTRAP.md)")?;
    ensure!(out.status.success(), "kubectl has no current context");
    Ok(String::from_utf8(out.stdout)
        .context("kubectl printed non-UTF-8")?
        .trim()
        .to_owned())
}

/// [`founder_confirms`] over its inputs: a refusal reads neither the context nor an answer, and
/// only the context typed exactly goes on.
fn confirm(
    ci: bool,
    terminal: bool,
    environment: Environment,
    action: &str,
    context: impl FnOnce() -> Result<String>,
    answer: impl FnOnce() -> Result<String>,
) -> Result<String> {
    if let Some(reason) = refusal(ci, terminal) {
        bail!("{reason} (DEC-2018)");
    }
    let context = context()?;
    eprint!(
        "    gitops: this will {action} the {} cluster through the context {context:?}.\n    Type the context's name to go on: ",
        environment.name()
    );
    ensure!(
        confirmed(&answer()?, &context),
        "not confirmed; nothing was applied"
    );
    Ok(context)
}

/// Runs `kubectl` against `cluster` with `stdin` as its input.
fn kubectl(cluster: &Cluster<'_>, args: &[&str], stdin: Option<&str>) -> Result<()> {
    let mut command = Command::new("kubectl");
    if let Some(kubeconfig) = cluster.kubeconfig {
        command.arg("--kubeconfig").arg(kubeconfig);
    }
    let mut child = command
        .args(["--v=0", "--context", &cluster.context])
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .spawn()
        .context("starting kubectl (deploy/gitops/BOOTSTRAP.md)")?;
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .context("kubectl's stdin")?
            .write_all(text.as_bytes())
            .context("handing kubectl its input")?;
    }
    let status = child.wait()?;
    ensure!(status.success(), "kubectl {} failed with {status}", args[0]);
    Ok(())
}

/// Writes `manifests`, after the workspace namespace, which Flux would otherwise create only on
/// its first reconcile.
fn write_secrets(cluster: &Cluster<'_>, manifests: &[(&str, String)]) -> Result<()> {
    let namespace = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {"name": WORKSPACE_NAMESPACE},
    });
    kubectl(
        cluster,
        &["apply", "--server-side", "-f", "-"],
        Some(&namespace.to_string()),
    )?;
    for (name, manifest) in manifests {
        kubectl(
            cluster,
            &["apply", "--server-side", "-f", "-"],
            Some(manifest),
        )?;
        eprintln!("    gitops: wrote the Secret {WORKSPACE_NAMESPACE}/{name}");
    }
    Ok(())
}

/// `cargo xtask gitops secrets <environment>`: the workload secrets alone.
pub(crate) fn secrets(environment: Environment) -> Result<()> {
    let manifests = WorkloadSecrets::from_env()?.manifests()?;
    let context = founder_confirms(environment, "write the workload secrets of")?;
    write_secrets(
        &Cluster {
            kubeconfig: None,
            context,
        },
        &manifests,
    )
}

/// What a bootstrap applies, read and checked before anything touches a cluster: the pinned Flux
/// controllers and the environment's `sync.yaml`.
fn prepare(root: &Path, environment: Environment) -> Result<(String, String)> {
    let sync = root
        .join("deploy/gitops/clusters")
        .join(environment.name())
        .join("sync.yaml");
    let sync = fs::read_to_string(&sync).with_context(|| format!("reading {}", sync.display()))?;
    let version = Command::new("flux")
        .args(["version", "--client"])
        .output()
        .context("starting flux (deploy/gitops/BOOTSTRAP.md)")?;
    let printed = String::from_utf8_lossy(&version.stdout);
    ensure!(
        printed.trim() == format!("flux: v{FLUX_VERSION}"),
        "flux {} is not the pinned v{FLUX_VERSION}",
        printed.trim()
    );
    let exported = Command::new("flux")
        .args(["install", "--export", "--components", FLUX_COMPONENTS])
        .output()
        .context("starting flux")?;
    ensure!(exported.status.success(), "flux install --export failed");
    let manifest =
        pin_images(&String::from_utf8(exported.stdout).context("flux printed non-UTF-8")?)?;
    Ok((manifest, sync))
}

/// Applies a bootstrap to `cluster`: the controllers, waiting for each; the workload secrets; then
/// `sync.yaml`. Idempotent, so every run re-asserts all three.
fn apply(
    cluster: &Cluster<'_>,
    environment: Environment,
    manifest: &str,
    secrets: &[(&str, String)],
    sync: &str,
) -> Result<()> {
    kubectl(
        cluster,
        &["apply", "--server-side", "--force-conflicts", "-f", "-"],
        Some(manifest),
    )?;
    for controller in [
        "source-controller",
        "kustomize-controller",
        "helm-controller",
    ] {
        kubectl(
            cluster,
            &[
                "rollout",
                "status",
                &format!("deployment/{controller}"),
                "--namespace",
                "flux-system",
                "--timeout=300s",
            ],
            None,
        )?;
    }
    write_secrets(cluster, secrets)?;
    kubectl(cluster, &["apply", "--server-side", "-f", "-"], Some(sync))?;
    eprintln!(
        "    gitops: {} bootstrapped; Flux now reconciles deploy/gitops/clusters/{} from main",
        environment.name(),
        environment.name()
    );
    Ok(())
}

/// `cargo xtask gitops bootstrap <environment>`, the founder's break-glass path: Flux's
/// controllers, pinned; the workload secrets; and the environment's `sync.yaml`. Everything is read
/// and checked before the founder confirms, so a refusal applies nothing.
pub(crate) fn bootstrap(root: &Path, environment: Environment) -> Result<()> {
    let (manifest, sync) = prepare(root, environment)?;
    let secrets = WorkloadSecrets::from_env()?.manifests()?;
    let context = founder_confirms(environment, "bootstrap Flux on")?;
    let cluster = Cluster {
        kubeconfig: None,
        context,
    };
    apply(&cluster, environment, &manifest, &secrets, &sync)
}

/// The bootstrap from CI (DEC-2018 item 7): lane IAC's `cargo xtask infra apply` calls this in
/// process, between opening and closing the runner's window on the control-plane firewall, with
/// the short-lived kubeconfig it fetched and the secrets from its sensitive outputs, all in memory.
/// It refuses unless this is `infra.yml` on `main` in GitHub Actions, and unless the kubeconfig's
/// current context is `expected_context`; then it applies exactly what the founder's path applies.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "lane IAC's `cargo xtask infra apply` calls it (E21-56 F2)"
    )
)]
pub(crate) fn bootstrap_from_infra(
    root: &Path,
    environment: Environment,
    kubeconfig: &Path,
    expected_context: &str,
    secrets: &WorkloadSecrets,
) -> Result<()> {
    let var = |name: &str| env::var(name).unwrap_or_default();
    if let Some(reason) = infra_refusal(
        &var("GITHUB_ACTIONS"),
        &var("GITHUB_REF"),
        &var("GITHUB_WORKFLOW_REF"),
    ) {
        bail!("{reason} (DEC-2018)");
    }
    let out = Command::new("kubectl")
        .args(["--v=0", "--kubeconfig"])
        .arg(kubeconfig)
        .args(["config", "current-context"])
        .output()
        .context("starting kubectl")?;
    ensure!(
        out.status.success(),
        "the kubeconfig has no current context"
    );
    let context = String::from_utf8(out.stdout)
        .context("kubectl printed non-UTF-8")?
        .trim()
        .to_owned();
    ensure!(
        confirmed(&context, expected_context),
        "the kubeconfig's context {context:?} is not {expected_context:?}; nothing was applied"
    );
    let (manifest, sync) = prepare(root, environment)?;
    let manifests = secrets.manifests()?;
    let cluster = Cluster {
        kubeconfig: Some(kubeconfig),
        context,
    };
    apply(&cluster, environment, &manifest, &manifests, &sync)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An excerpt of `flux install --export` for v2.9.6, its image lines as the CLI prints them.
    fn exported() -> String {
        FLUX_IMAGES
            .iter()
            .map(|(tag, _)| {
                format!("kind: Deployment\nspec:\n      containers:\n      - name: manager\n        image: {tag}\n")
            })
            .collect::<Vec<_>>()
            .join("---\n")
    }

    #[test]
    fn every_flux_image_is_pinned_by_its_digest() -> Result<()> {
        let pinned = pin_images(&exported())?;
        for (tag, digest) in FLUX_IMAGES {
            assert!(
                pinned.contains(&format!("        image: {tag}@{digest}\n")),
                "{pinned}"
            );
        }
        assert!(
            !pinned
                .lines()
                .any(|line| line.contains("image: ") && !line.contains("@sha256:"))
        );
        Ok(())
    }

    #[test]
    fn an_image_not_pinned_here_or_a_missing_one_is_refused() {
        let unknown = exported().replace("helm-controller:v1.6.5", "helm-controller:v1.7.0");
        assert!(pin_images(&unknown).is_err());
        let extra = format!(
            "{}---\n      - image: ghcr.io/fluxcd/notification-controller:v1.9.4\n",
            exported()
        );
        assert!(pin_images(&extra).is_err());
        let fewer = exported().replace("image: ghcr.io/fluxcd/source-controller:v1.9.6", "name: x");
        assert!(pin_images(&fewer).is_err());
    }

    #[test]
    fn a_secret_holds_its_values_base64_in_the_workspace_namespace() -> Result<()> {
        let text = secret_manifest("cloudflared-tunnel", &[("token", "abc".to_owned())])?;
        let secret: serde_json::Value = serde_json::from_str(&text)?;
        assert_eq!(secret["metadata"]["namespace"], WORKSPACE_NAMESPACE);
        assert_eq!(secret["metadata"]["name"], "cloudflared-tunnel");
        assert_eq!(secret["data"]["token"], "YWJj");
        assert!(!text.contains("\"abc\""));
        assert!(secret_manifest("cloudflared-tunnel", &[("token", String::new())]).is_err());
        Ok(())
    }

    /// The Secrets written are exactly those the overlays' values name: the archive's key and the
    /// tunnel's token, with the keys the charts read.
    #[test]
    fn the_secrets_written_are_the_ones_the_charts_read() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let journal = fs::read_to_string(root.join("deploy/cell/charts/journal-db/values.yaml"))?;
        assert!(journal.contains("keyFrom: journal-archive-key"));
        assert!(journal.contains("`ACCESS_KEY_ID` and `ACCESS_SECRET_KEY`"));
        let managed = fs::read_to_string(root.join("deploy/cell/values/managed.yaml"))?;
        assert!(managed.contains("tokenFrom: cloudflared-tunnel"));
        let names: Vec<&str> = WORKLOAD_SECRETS.iter().map(|secret| secret.name).collect();
        assert_eq!(names, ["journal-archive-key", "cloudflared-tunnel"]);
        Ok(())
    }

    #[test]
    fn only_the_two_hosted_environments_parse() -> Result<()> {
        assert_eq!(Environment::parse("staging")?, Environment::Staging);
        assert_eq!(Environment::parse("production")?, Environment::Production);
        assert!(Environment::parse("dev").is_err());
        assert!(Environment::parse("Staging").is_err());
        Ok(())
    }

    /// Nothing runs in CI or without a terminal, so an agent's shell can never apply anything.
    #[test]
    fn only_the_founders_terminal_outside_ci_may_apply() {
        assert!(refusal(true, true).is_some());
        assert!(refusal(true, false).is_some());
        assert!(refusal(false, false).is_some());
        assert!(refusal(false, true).is_none());
    }

    #[test]
    fn only_the_exact_context_confirms() {
        assert!(confirmed(
            "do-nyc3-mandate-staging\n",
            "do-nyc3-mandate-staging"
        ));
        assert!(!confirmed("yes\n", "do-nyc3-mandate-staging"));
        assert!(!confirmed(
            "do-nyc3-mandate-staging-old\n",
            "do-nyc3-mandate-staging"
        ));
        assert!(!confirmed("\n", ""));
    }

    /// The confirmation refuses in CI and without a terminal before it reads anything, and goes on
    /// only on the exact context.
    #[test]
    fn the_confirmation_refuses_before_reading_and_goes_on_only_on_the_context() {
        let context = "do-nyc3-mandate-staging";
        let never = || -> Result<String> { bail!("read after a refusal") };
        for (ci, terminal) in [(true, true), (true, false), (false, false)] {
            let refused = confirm(
                ci,
                terminal,
                Environment::Staging,
                "bootstrap",
                never,
                never,
            );
            assert!(refused.is_err_and(|e| e.to_string().contains("DEC-2018")));
        }
        let wrong = confirm(
            false,
            true,
            Environment::Staging,
            "bootstrap",
            || Ok(context.to_owned()),
            || Ok("yes\n".to_owned()),
        );
        assert!(wrong.is_err());
        let right = confirm(
            false,
            true,
            Environment::Staging,
            "bootstrap",
            || Ok(context.to_owned()),
            || Ok(format!("{context}\n")),
        );
        assert!(right.is_ok_and(|c| c == context));
    }

    /// Only `infra.yml` on `main` in GitHub Actions may bootstrap from CI.
    #[test]
    fn only_infra_yml_on_main_may_bootstrap_from_ci() {
        let infra = "kunwarshivam/mandate/.github/workflows/infra.yml@refs/heads/main";
        assert!(infra_refusal("true", "refs/heads/main", infra).is_none());
        assert!(infra_refusal("", "refs/heads/main", infra).is_some());
        assert!(infra_refusal("true", "refs/heads/go", infra).is_some());
        assert!(infra_refusal("true", "refs/pull/1/merge", infra).is_some());
        let other = "kunwarshivam/mandate/.github/workflows/ci.yml@refs/heads/main";
        assert!(infra_refusal("true", "refs/heads/main", other).is_some());
        let branch = "kunwarshivam/mandate/.github/workflows/infra.yml@refs/heads/go";
        assert!(infra_refusal("true", "refs/heads/main", branch).is_some());
    }

    /// The CI bootstrap refuses outside infra.yml before it reads the kubeconfig or the cluster.
    #[test]
    fn the_ci_bootstrap_refuses_before_it_reads_anything() {
        let secrets = WorkloadSecrets {
            archive_access_key_id: Secret::new("id".to_owned()),
            archive_secret_access_key: Secret::new("secret".to_owned()),
            tunnel_token: Secret::new("token".to_owned()),
        };
        let refused = bootstrap_from_infra(
            Path::new("/nonexistent"),
            Environment::Staging,
            Path::new("/nonexistent/kubeconfig"),
            "do-nyc3-mandate-staging",
            &secrets,
        );
        assert!(refused.is_err_and(|e| e.to_string().contains("DEC-2018")));
    }

    /// The secrets' `Debug` never shows a value, and each value lands in its chart's key.
    #[test]
    fn the_secrets_are_redacted_and_land_in_their_keys() -> Result<()> {
        let secrets = WorkloadSecrets {
            archive_access_key_id: Secret::new("key-id-1".to_owned()),
            archive_secret_access_key: Secret::new("secret-2".to_owned()),
            tunnel_token: Secret::new("token-3".to_owned()),
        };
        let shown = format!("{secrets:?}");
        for value in ["key-id-1", "secret-2", "token-3"] {
            assert!(!shown.contains(value), "{shown}");
        }
        let manifests = secrets.manifests()?;
        let data = |name: &str| -> Result<serde_json::Value> {
            let (_, text) = manifests
                .iter()
                .find(|(n, _)| *n == name)
                .context("missing")?;
            Ok(serde_json::from_str::<serde_json::Value>(text)?["data"].clone())
        };
        let archive = data("journal-archive-key")?;
        assert_eq!(archive["ACCESS_KEY_ID"], image::base64(b"key-id-1"));
        assert_eq!(archive["ACCESS_SECRET_KEY"], image::base64(b"secret-2"));
        assert_eq!(
            data("cloudflared-tunnel")?["token"],
            image::base64(b"token-3")
        );
        Ok(())
    }

    /// Every printable form of a secret is masked, each on one line, none empty or repeated.
    #[test]
    fn every_printable_form_of_a_secret_is_masked() {
        let value = "a+b/c=d\"e\nline two";
        let forms = mask_forms(value);
        let base64 = image::base64(value.as_bytes());
        for expected in [
            base64.clone(),
            base64
                .replace('+', "-")
                .replace('/', "_")
                .trim_end_matches('=')
                .to_owned(),
            "a%2Bb%2Fc%3Dd%22e%0Aline%20two".to_owned(),
            "a+b/c=d\\\"e\\nline two".to_owned(),
            "a+b/c=d\"e".to_owned(),
            "line two".to_owned(),
        ] {
            assert!(forms.contains(&expected), "{expected} not in {forms:?}");
        }
        assert!(forms.iter().all(|f| !f.is_empty() && !f.contains('\n')));
        let mut unique = forms.clone();
        unique.dedup();
        assert_eq!(unique.len(), forms.len());
        assert_eq!(
            mask_forms("plain"),
            vec![
                "plain".to_owned(),
                image::base64(b"plain"),
                image::base64(b"plain").trim_end_matches('=').to_owned()
            ]
        );
        assert_eq!(
            format!("{:?}", Secret::new("hidden".to_owned())),
            "<redacted>"
        );
    }

    /// No workflow may run the bootstrap: the cluster's API admits only the founder's addresses.
    #[test]
    fn no_workflow_runs_the_bootstrap() -> Result<()> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows");
        for entry in fs::read_dir(dir)? {
            let text = fs::read_to_string(entry?.path())?;
            assert!(!text.contains("xtask gitops bootstrap"));
            assert!(!text.contains("xtask gitops secrets"));
        }
        Ok(())
    }
}
