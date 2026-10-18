//! `cargo xtask gitops`: the founder's one bootstrap of a hosted cluster, and its workload secrets
//! (E21-58, DEC-2015, DEC-2018).
//!
//! Both commands run only from `.github/workflows/gitops-bootstrap.yml`, dispatched from `main`
//! under the environment's GitHub environment, whose required reviewer is the founder. They refuse
//! to run anywhere else, so an agent never applies anything to a real cluster (DEC-2015 item 5).
//! The cluster's credential and the workload secrets arrive as that environment's secrets in the
//! process environment. Each is written only to a 0600 file or to `kubectl`'s stdin, and never
//! printed.
//!
//! The environment is the dispatch's choice, from `GITOPS_ENVIRONMENT`.
//! `gitops bootstrap` installs Flux's three controllers at the pinned version, each
//! image pinned by digest, writes the workload secrets, and applies the environment's `sync.yaml`.
//! From then on, Flux reconciles `deploy/gitops/clusters/<environment>` from `main`.
//! `gitops secrets` writes the workload secrets alone, to set or rotate them.

use std::env;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
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

    /// The environment the workflow's dispatch chose, from `GITOPS_ENVIRONMENT`.
    pub(crate) fn from_env() -> Result<Self> {
        Self::parse(&env::var("GITOPS_ENVIRONMENT").context("GITOPS_ENVIRONMENT is not set")?)
    }

    fn name(self) -> &'static str {
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

/// The environment secret holding the cluster's kubeconfig.
const KUBECONFIG_VAR: &str = "KUBECONFIG_DATA";

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

/// Refuses to run outside a GitHub Actions run of `main`: only the founder's approved workflow
/// may touch a hosted cluster (DEC-2015 item 5).
fn approved_run() -> Result<()> {
    let actions = env::var("GITHUB_ACTIONS").unwrap_or_default();
    let reference = env::var("GITHUB_REF").unwrap_or_default();
    ensure!(
        actions == "true" && reference == "refs/heads/main",
        "`cargo xtask gitops` runs only from gitops-bootstrap.yml on main, under the environment's \
         required reviewers (DEC-2018); never by hand or from an agent"
    );
    Ok(())
}

/// The cluster's kubeconfig from [`KUBECONFIG_VAR`], written to a 0600 file in a 0700 directory
/// under `out`, which the caller deletes.
fn kubeconfig(out: &Path) -> Result<PathBuf> {
    let data = env::var(KUBECONFIG_VAR).with_context(|| format!("{KUBECONFIG_VAR} is not set"))?;
    ensure!(!data.trim().is_empty(), "{KUBECONFIG_VAR} is empty");
    fs::create_dir_all(out).context("creating the kubeconfig directory")?;
    fs::set_permissions(out, fs::Permissions::from_mode(0o700))?;
    let path = out.join("kubeconfig");
    fs::write(&path, data).context("writing the kubeconfig")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(path)
}

/// Runs `kubectl` against `config` with `stdin` as its input.
fn kubectl(config: &Path, args: &[&str], stdin: Option<&str>) -> Result<()> {
    let mut child = Command::new("kubectl")
        .arg("--kubeconfig")
        .arg(config)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .spawn()
        .context("starting kubectl (gitops-bootstrap.yml installs it)")?;
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

/// Writes every [`WORKLOAD_SECRETS`] entry from its environment secrets, after the workspace
/// namespace, which Flux would otherwise create only on its first reconcile.
fn write_secrets(config: &Path) -> Result<()> {
    let namespace = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {"name": WORKSPACE_NAMESPACE},
    });
    kubectl(
        config,
        &["apply", "--server-side", "-f", "-"],
        Some(&namespace.to_string()),
    )?;
    for secret in WORKLOAD_SECRETS {
        let mut values = Vec::new();
        for (key, var) in secret.keys {
            let value = env::var(var).with_context(|| format!("{var} is not set"))?;
            values.push((*key, value));
        }
        let manifest = secret_manifest(secret.name, &values)?;
        kubectl(
            config,
            &["apply", "--server-side", "-f", "-"],
            Some(&manifest),
        )?;
        eprintln!(
            "    gitops: wrote the Secret {WORKSPACE_NAMESPACE}/{}",
            secret.name
        );
    }
    Ok(())
}

/// `cargo xtask gitops secrets`: the workload secrets alone.
pub(crate) fn secrets(root: &Path, environment: Environment) -> Result<()> {
    approved_run()?;
    let dir = root.join("target").join("gitops").join(environment.name());
    let config = kubeconfig(&dir)?;
    let written = write_secrets(&config);
    fs::remove_dir_all(&dir).context("deleting the kubeconfig")?;
    written
}

/// `cargo xtask gitops bootstrap`: Flux's controllers, pinned; the workload
/// secrets; and the environment's `sync.yaml`, after which Flux reconciles the rest.
pub(crate) fn bootstrap(root: &Path, environment: Environment) -> Result<()> {
    approved_run()?;
    let sync = root
        .join("deploy/gitops/clusters")
        .join(environment.name())
        .join("sync.yaml");
    ensure!(sync.is_file(), "{} is missing", sync.display());
    let exported = Command::new("flux")
        .args(["install", "--export", "--components", FLUX_COMPONENTS])
        .output()
        .context("starting flux (gitops-bootstrap.yml installs it)")?;
    ensure!(exported.status.success(), "flux install --export failed");
    let manifest =
        pin_images(&String::from_utf8(exported.stdout).context("flux printed non-UTF-8")?)?;
    let dir = root.join("target").join("gitops").join(environment.name());
    let config = kubeconfig(&dir)?;
    let applied = (|| {
        kubectl(
            &config,
            &["apply", "--server-side", "--force-conflicts", "-f", "-"],
            Some(&manifest),
        )?;
        for controller in [
            "source-controller",
            "kustomize-controller",
            "helm-controller",
        ] {
            kubectl(
                &config,
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
        write_secrets(&config)?;
        let sync = fs::read_to_string(&sync).context("reading sync.yaml")?;
        kubectl(&config, &["apply", "--server-side", "-f", "-"], Some(&sync))
    })();
    fs::remove_dir_all(&dir).context("deleting the kubeconfig")?;
    applied?;
    eprintln!(
        "    gitops: {} bootstrapped; Flux now reconciles deploy/gitops/clusters/{} from main",
        environment.name(),
        environment.name()
    );
    Ok(())
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

    /// The bootstrap workflow runs only by dispatch, only on `main`, only under the chosen
    /// environment's reviewers, and reads no pull request (ES-12, DEC-1005, DEC-2018).
    #[test]
    fn the_bootstrap_workflow_runs_only_from_main_under_its_environment() -> Result<()> {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/gitops-bootstrap.yml");
        let text = fs::read_to_string(path)?;
        let on = text
            .split_once("\non:\n")
            .and_then(|(_, rest)| rest.split_once("\npermissions:"))
            .map(|(on, _)| on)
            .context("no on: block")?;
        assert!(on.starts_with("  workflow_dispatch:\n"), "{on}");
        for trigger in [
            "pull_request",
            "push:",
            "workflow_run",
            "schedule",
            "issue_comment",
        ] {
            assert!(!on.contains(trigger), "{trigger}");
        }
        assert!(text.contains("\npermissions: {}\n"));
        assert!(text.contains("    if: github.ref == 'refs/heads/main'\n"));
        assert!(text.contains("    environment: ${{ inputs.environment }}\n"));
        assert!(
            !text.contains(": write"),
            "the workflow grants no write permission"
        );
        Ok(())
    }
}
