//! `cargo xtask gitops`: the founder's one bootstrap of a hosted cluster, and its workload secrets
//! (E21-58, DEC-2015, DEC-2018).
//!
//! The cluster's control-plane firewall admits only the founder's addresses (lane IAC's
//! `KUBE_API_ALLOWED_CIDRS`), so these commands run on the founder's machine, with the founder's
//! kubeconfig, and nowhere else. They refuse to run in CI or without a terminal, and they apply
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

/// Runs `kubectl` against `context` with `stdin` as its input.
fn kubectl(context: &str, args: &[&str], stdin: Option<&str>) -> Result<()> {
    let mut child = Command::new("kubectl")
        .args(["--context", context])
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

/// Reads every [`WORKLOAD_SECRETS`] value from the process environment, each required.
fn secret_manifests() -> Result<Vec<(&'static str, String)>> {
    let mut manifests = Vec::new();
    for secret in WORKLOAD_SECRETS {
        let mut values = Vec::new();
        for (key, var) in secret.keys {
            let value =
                env::var(var).with_context(|| format!("{var} is not set (BOOTSTRAP.md)"))?;
            values.push((*key, value));
        }
        manifests.push((secret.name, secret_manifest(secret.name, &values)?));
    }
    Ok(manifests)
}

/// Writes `manifests`, after the workspace namespace, which Flux would otherwise create only on
/// its first reconcile.
fn write_secrets(context: &str, manifests: &[(&str, String)]) -> Result<()> {
    let namespace = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {"name": WORKSPACE_NAMESPACE},
    });
    kubectl(
        context,
        &["apply", "--server-side", "-f", "-"],
        Some(&namespace.to_string()),
    )?;
    for (name, manifest) in manifests {
        kubectl(
            context,
            &["apply", "--server-side", "-f", "-"],
            Some(manifest),
        )?;
        eprintln!("    gitops: wrote the Secret {WORKSPACE_NAMESPACE}/{name}");
    }
    Ok(())
}

/// `cargo xtask gitops secrets <environment>`: the workload secrets alone.
pub(crate) fn secrets(environment: Environment) -> Result<()> {
    let manifests = secret_manifests()?;
    let context = founder_confirms(environment, "write the workload secrets of")?;
    write_secrets(&context, &manifests)
}

/// `cargo xtask gitops bootstrap <environment>`: Flux's controllers, pinned; the workload
/// secrets; and the environment's `sync.yaml`, after which Flux reconciles the rest. Everything
/// is read and checked before the founder confirms, so a refusal applies nothing.
pub(crate) fn bootstrap(root: &Path, environment: Environment) -> Result<()> {
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
    let secrets = secret_manifests()?;
    let context = founder_confirms(environment, "bootstrap Flux on")?;
    kubectl(
        &context,
        &["apply", "--server-side", "--force-conflicts", "-f", "-"],
        Some(&manifest),
    )?;
    for controller in [
        "source-controller",
        "kustomize-controller",
        "helm-controller",
    ] {
        kubectl(
            &context,
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
    write_secrets(&context, &secrets)?;
    kubectl(
        &context,
        &["apply", "--server-side", "-f", "-"],
        Some(&sync),
    )?;
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
