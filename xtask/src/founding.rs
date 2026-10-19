//! Staging's founder linked from CI (DEC-2114): after the vault is provisioned, inside the same
//! window, staging's apply runs the `mandate` binary the job built with no secret in its
//! environment, `identity link`, with the founder's identity-provider subject on its standard input
//! alone, and applies the one-line file it writes, `sha256:<digest> <ULID>`, as the
//! `subject-links` ConfigMap the API server mounts, through `kubectl`'s standard input. The subject
//! is masked the moment it is read and is never an argument, an environment variable, a file, a
//! cluster object, or a log line: only its digest reaches the cluster. The founding grant itself is
//! the workspace chart's `founding` hook, which carries no subject at all.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, ensure};
use serde_json::json;

use crate::gitops::WORKSPACE_NAMESPACE;
use crate::vault_init::Kubectl;

/// Staging's workspace values, which name the issuer and the founder's principal.
pub(crate) const STAGING_VALUES: &str = "deploy/gitops/clusters/staging/workspace.yaml";
/// The ConfigMap the API server mounts at its subject-link path, and its one key.
pub(crate) const CONFIGMAP: &str = "subject-links";
/// The variable the apply step alone sets, from the `infra-staging` secret `FOUNDER_OIDC_SUBJECT`.
pub(crate) const SUBJECT_VAR: &str = "MANDATE_FOUNDER_OIDC_SUBJECT";

/// The founder as staging's values name them: the identity provider's issuer and the principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Founder {
    pub(crate) issuer: String,
    pub(crate) member: String,
}

/// The issuer (`api.issuer`) and the founder's principal (`founding.member`) of staging's values.
///
/// # Errors
/// When the file cannot be read or names either value as nothing.
pub(crate) fn staging_founder(root: &Path) -> Result<Founder> {
    let text = fs::read_to_string(root.join(STAGING_VALUES))
        .with_context(|| format!("reading {STAGING_VALUES}"))?;
    Ok(Founder {
        issuer: value_in(&text, "api", "issuer")?,
        member: value_in(&text, "founding", "member")?,
    })
}

/// The scalar `key` of the mapping `block` in `text`: the first line `<indent>key: value` after the
/// line `<indent>block:` that is indented deeper than it, before any line that is not.
fn value_in(text: &str, block: &str, key: &str) -> Result<String> {
    let mut lines = text.lines();
    let opening = lines
        .by_ref()
        .find(|line| line.trim_start() == format!("{block}:"))
        .with_context(|| format!("{STAGING_VALUES} has no `{block}:` block"))?;
    let depth = opening.len() - opening.trim_start().len();
    let prefix = format!("{key}: ");
    lines
        .take_while(|line| line.trim().is_empty() || line.len() - line.trim_start().len() > depth)
        .find_map(|line| line.trim_start().strip_prefix(&prefix))
        .map(|value| value.trim().trim_matches('"').to_owned())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("{STAGING_VALUES} names no `{block}.{key}`"))
}

/// What the step needs: `kubectl` against staging, `mandate` run with the given arguments and
/// standard input answering whether it succeeded, and a directory of the runner's own for the one
/// file it writes.
pub(crate) struct Steps<'a> {
    pub(crate) kubectl: Kubectl<'a>,
    pub(crate) mandate: &'a dyn Fn(&[String], &str) -> Result<bool>,
    pub(crate) scratch: &'a Path,
}

/// `mandate identity link`'s arguments for `founder`, writing `links`; the subject is no argument.
pub(crate) fn link_command(founder: &Founder, links: &Path) -> Vec<String> {
    vec![
        "identity".to_owned(),
        "link".to_owned(),
        "--issuer".to_owned(),
        founder.issuer.clone(),
        "--member".to_owned(),
        founder.member.clone(),
        "--links".to_owned(),
        links.display().to_string(),
    ]
}

/// Links `founder`'s `subject` and applies the link as the `subject-links` ConfigMap, answering the
/// word to print: `skipped` when no subject is set, so a run without the secret changes nothing,
/// and `linked` once the ConfigMap is applied. The file is written in a fresh directory under the
/// scratch directory and removed before the apply.
///
/// # Errors
/// When `mandate identity link` refuses (its reason goes to its own standard error, never here),
/// when its file cannot be read, or when `kubectl apply` fails.
pub(crate) fn link_founder(
    steps: &Steps<'_>,
    founder: &Founder,
    subject: &str,
) -> Result<&'static str> {
    if subject.is_empty() {
        return Ok("skipped: no founder subject is set");
    }
    let dir = fresh_dir(steps.scratch)?;
    let links = dir.join(CONFIGMAP);
    let linked = (steps.mandate)(&link_command(founder, &links), subject);
    let text = linked.and_then(|ok| {
        ensure!(ok, "`mandate identity link` refused the founder's link");
        fs::read_to_string(&links).context("reading the link file `mandate` wrote")
    });
    let removed = fs::remove_dir_all(&dir).context("removing the link file's directory");
    let text = text?;
    removed?;
    let (applied, _) = (steps.kubectl)(&["apply", "-f", "-"], Some(&configmap(&text)))?;
    ensure!(applied, "kubectl apply of the {CONFIGMAP} ConfigMap failed");
    Ok("linked")
}

/// A directory of its own under `scratch`, emptied first.
fn fresh_dir(scratch: &Path) -> Result<PathBuf> {
    let dir = scratch.join("founding");
    if dir.exists() {
        fs::remove_dir_all(&dir).context("emptying the founding directory")?;
    }
    fs::create_dir_all(&dir).context("making the founding directory")?;
    Ok(dir)
}

/// The `subject-links` ConfigMap holding `text`, in the workspace's namespace.
fn configmap(text: &str) -> String {
    json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
            "name": CONFIGMAP,
            "namespace": WORKSPACE_NAMESPACE,
            "labels": {"app.kubernetes.io/managed-by": "mandate-founding"},
        },
        "data": {CONFIGMAP: text},
    })
    .to_string()
}

/// Runs the built `mandate` at `cli` with `args`, an empty environment, `stdin` on its standard
/// input, and its output discarded, answering whether it succeeded.
///
/// # Errors
/// When it cannot be started or written to.
pub(crate) fn run_mandate(cli: &Path, args: &[String], stdin: &str) -> Result<bool> {
    let mut child = Command::new(cli)
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting the built mandate")?;
    child
        .stdin
        .take()
        .context("mandate's stdin")?
        .write_all(stdin.as_bytes())?;
    Ok(child.wait().context("waiting for mandate")?.success())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use anyhow::{Result, bail};

    use super::{CONFIGMAP, Founder, Steps, link_command, link_founder, staging_founder, value_in};

    const CANARY: &str = "canary-subject-7f3c2b19-e4a1";
    const DIGEST: &str = "sha256:0d1e2f3a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f6";

    fn founder() -> Founder {
        Founder {
            issuer: "https://issuer.example/auth/v1".to_owned(),
            member: "01KDEM0000000000000000000F".to_owned(),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("xtask-founding-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    /// What the fakes saw: each `mandate` call's arguments and standard input, and each `kubectl`
    /// call's arguments and standard input.
    #[derive(Default)]
    struct Seen {
        mandate: Vec<(Vec<String>, String)>,
        kubectl: Vec<(Vec<String>, Option<String>)>,
    }

    /// Runs the step with fakes: `mandate` writes the link file the CLI would, a digest and the
    /// member, when `links_ok`; `kubectl` answers `applies`.
    fn run(subject: &str, links_ok: bool, applies: bool) -> (Result<&'static str>, Seen, PathBuf) {
        let seen = RefCell::new(Seen::default());
        let dir = scratch(&format!("{links_ok}-{applies}-{}", subject.len()));
        let mandate = |args: &[String], stdin: &str| -> Result<bool> {
            seen.borrow_mut()
                .mandate
                .push((args.to_vec(), stdin.to_owned()));
            let links = args
                .iter()
                .position(|a| a == "--links")
                .and_then(|i| args.get(i + 1));
            match (links_ok, links) {
                (true, Some(path)) => {
                    std::fs::write(path, format!("{DIGEST} {}\n", founder().member))?;
                    Ok(true)
                }
                (true, None) => bail!("no --links"),
                (false, _) => Ok(false),
            }
        };
        let kubectl = |args: &[&str], stdin: Option<&str>| -> Result<(bool, String)> {
            seen.borrow_mut().kubectl.push((
                args.iter().map(|a| (*a).to_owned()).collect(),
                stdin.map(str::to_owned),
            ));
            Ok((applies, String::new()))
        };
        let steps = Steps {
            kubectl: &kubectl,
            mandate: &mandate,
            scratch: &dir,
        };
        let got = link_founder(&steps, &founder(), subject);
        (got, seen.into_inner(), dir)
    }

    #[test]
    fn the_subject_reaches_mandate_on_stdin_alone_and_only_its_digest_reaches_the_cluster() {
        let (got, seen, dir) = run(CANARY, true, true);
        assert_eq!(got.expect("linked"), "linked");
        assert_eq!(seen.mandate.len(), 1);
        let (args, stdin) = &seen.mandate[0];
        assert_eq!(stdin, CANARY, "the subject is mandate's standard input");
        assert!(!args.iter().any(|a| a.contains(CANARY)), "{args:?}");
        assert_eq!(&args[..6], &link_command(&founder(), Path::new("x"))[..6]);
        assert_eq!(seen.kubectl.len(), 1);
        let (kargs, manifest) = &seen.kubectl[0];
        assert_eq!(kargs, &["apply", "-f", "-"], "no argument carries the link");
        let manifest = manifest.as_deref().expect("the manifest on stdin");
        assert!(!manifest.contains(CANARY), "{manifest}");
        let object: serde_json::Value = serde_json::from_str(manifest).expect("JSON");
        assert_eq!(object["kind"], "ConfigMap");
        assert_eq!(object["metadata"]["name"], CONFIGMAP);
        assert_eq!(object["metadata"]["namespace"], "ws-ws1");
        assert_eq!(
            object["data"][CONFIGMAP],
            format!("{DIGEST} {}\n", founder().member)
        );
        let left: Vec<_> = walk(&dir);
        assert!(left.is_empty(), "the link file is removed: {left:?}");
    }

    #[test]
    fn no_subject_runs_nothing_and_changes_nothing() {
        let (got, seen, _) = run("", true, true);
        assert_eq!(got.expect("skipped"), "skipped: no founder subject is set");
        assert!(seen.mandate.is_empty() && seen.kubectl.is_empty());
    }

    #[test]
    fn a_refused_link_applies_nothing_and_a_failed_apply_fails_the_step() {
        let (got, seen, dir) = run(CANARY, false, true);
        let error = got.expect_err("refused").to_string();
        assert!(!error.contains(CANARY), "{error}");
        assert!(seen.kubectl.is_empty(), "nothing applied");
        assert!(walk(&dir).is_empty());
        let (got, seen, _) = run(CANARY, true, false);
        assert!(got.is_err());
        assert_eq!(seen.kubectl.len(), 1);
    }

    #[test]
    fn the_founder_is_read_from_staging_s_values() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let read = staging_founder(&root).expect("staging names its founder");
        assert_eq!(
            read.issuer,
            "https://wyyxngmblqqokcvraflx.supabase.co/auth/v1"
        );
        assert_eq!(read.member, "01KDEM0000000000000000000F");
        let text = "spec:\n  values:\n    api:\n      org: o\n      issuer: \"https://i\"\n    founding:\n      enabled: false\n";
        assert_eq!(
            value_in(text, "api", "issuer").expect("issuer"),
            "https://i"
        );
        assert!(value_in(text, "founding", "member").is_err(), "absent");
        assert!(value_in(text, "missing", "issuer").is_err(), "no block");
        let later = "    api:\n      org: o\n    other:\n      issuer: x\n";
        assert!(value_in(later, "api", "issuer").is_err(), "not the block's");
        let empty = "    founding:\n      member: \"\"\n";
        assert!(value_in(empty, "founding", "member").is_err(), "empty");
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .flat_map(|e| {
                        let path = e.path();
                        if path.is_dir() {
                            walk(&path)
                        } else {
                            vec![path]
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
