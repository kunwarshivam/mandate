//! Staging's founder linked from CI (DEC-2114): after the vault is provisioned, inside the same
//! window, staging's apply runs the `mandate` binary the job built with no secret in its
//! environment, `identity link`, with the founder's identity-provider subject on its standard input
//! alone, and applies the one-line file it writes, `sha256:<digest> <ULID>`, as the
//! `subject-links` ConfigMap the API server mounts, through `kubectl`'s standard input, empty when no
//! subject is set; a changed link restarts the API server, whose `subPath` mount never refreshes
//! (DEC-2114 item 2). The subject
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

/// The hosted workspace (`api.workspace`) of staging's values, whose control stream a step-up
/// confirmation is appended to (DEC-2087).
///
/// # Errors
/// When the file cannot be read or names no workspace.
pub(crate) fn staging_workspace(root: &Path) -> Result<String> {
    let text = fs::read_to_string(root.join(STAGING_VALUES))
        .with_context(|| format!("reading {STAGING_VALUES}"))?;
    value_in(&text, "api", "workspace")
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

/// Links `founder`'s `subject` and applies the link as the `subject-links` ConfigMap on every run:
/// with no subject set the ConfigMap is applied empty, so the secret alone decides who is linked.
/// The API server mounts the file by `subPath`, which the kubelet never refreshes, so when the
/// applied content differs from what was there and the `api` Deployment exists, it is restarted.
/// The ConfigMap's earlier content is read to compare and never printed. Answers what it did, in
/// fixed words. The link file is written in a fresh directory under the scratch directory and
/// removed before the apply.
///
/// # Errors
/// When `mandate identity link` refuses (its reason goes to its own standard error, never here),
/// when its file cannot be read, or when `kubectl` fails to apply or to restart.
pub(crate) fn link_founder(steps: &Steps<'_>, founder: &Founder, subject: &str) -> Result<String> {
    let text = if subject.is_empty() {
        String::new()
    } else {
        linked_text(steps, founder, subject)?
    };
    let before = current(steps)?;
    let (applied, _) = (steps.kubectl)(&["apply", "-f", "-"], Some(&configmap(&text)))?;
    ensure!(applied, "kubectl apply of the {CONFIGMAP} ConfigMap failed");
    let what = if subject.is_empty() {
        "no founder subject is set: nobody is linked"
    } else {
        "linked"
    };
    if before.as_deref() == Some(text.as_str()) {
        return Ok(format!("{what}, unchanged"));
    }
    let (api, _) = (steps.kubectl)(
        &[
            "get",
            "deployment",
            "api",
            "--namespace",
            WORKSPACE_NAMESPACE,
            "-o",
            "name",
        ],
        None,
    )?;
    if !api {
        return Ok(what.to_owned());
    }
    let (restarted, _) = (steps.kubectl)(
        &[
            "rollout",
            "restart",
            "deployment/api",
            "--namespace",
            WORKSPACE_NAMESPACE,
        ],
        None,
    )?;
    ensure!(restarted, "kubectl could not restart the api Deployment");
    Ok(format!("{what}; api restarted"))
}

/// The link file `mandate identity link` writes for `founder` and `subject`, read and removed.
fn linked_text(steps: &Steps<'_>, founder: &Founder, subject: &str) -> Result<String> {
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
    Ok(text)
}

/// The `subject-links` ConfigMap's content now, or `None` when there is none.
fn current(steps: &Steps<'_>) -> Result<Option<String>> {
    let (found, json) = (steps.kubectl)(
        &[
            "get",
            "configmap",
            CONFIGMAP,
            "--namespace",
            WORKSPACE_NAMESPACE,
            "-o",
            "json",
        ],
        None,
    )?;
    if !found {
        return Ok(None);
    }
    let object: serde_json::Value =
        serde_json::from_str(&json).context("the subject-links ConfigMap is not JSON")?;
    Ok(Some(
        object["data"][CONFIGMAP]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    ))
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

/// The secrets the infra step's own environment holds under a `MANDATE_` name, each read once by
/// the step that needs it and handed on only on a child's standard input (DEC-2114, DEC-2630,
/// DEC-2636).
pub(crate) const STEP_SECRETS: [&str; 3] = [
    SUBJECT_VAR,
    "MANDATE_OPENROUTER_API_KEY",
    "MANDATE_ALPACA_OAUTH_CLIENT_SECRET",
];

/// `command` with none of [`STEP_SECRETS`] in its environment, for a child that inherits the
/// step's environment rather than starting from an empty one.
pub(crate) fn without_step_secrets(command: &mut Command) -> &mut Command {
    for name in STEP_SECRETS {
        command.env_remove(name);
    }
    command
}

/// Runs the built `mandate` at `cli` with `args`, an empty environment, `stdin` on its standard
/// input, and its output discarded, answering whether it succeeded. It is waited for even when
/// writing its input fails, so its exit is always reaped.
///
/// # Errors
/// When it cannot be started, written to, or waited for.
pub(crate) fn run_mandate(cli: &Path, args: &[String], stdin: &str) -> Result<bool> {
    let mut child = Command::new(cli)
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting the built mandate")?;
    let written = child
        .stdin
        .take()
        .context("mandate's stdin")
        .and_then(|mut input| Ok(input.write_all(stdin.as_bytes())?));
    let status = child.wait().context("waiting for mandate")?;
    written?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use anyhow::{Result, bail};

    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    use super::{
        CONFIGMAP, Founder, STEP_SECRETS, Steps, link_command, link_founder, run_mandate,
        staging_founder, value_in, without_step_secrets,
    };

    const CANARY: &str = "canary-subject-7f3c2b19-e4a1";
    const DIGEST: &str = "sha256:0d1e2f3a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f6";

    fn founder() -> Founder {
        Founder {
            issuer: "https://issuer.example/auth/v1".to_owned(),
            member: "01KDEM0000000000000000000F".to_owned(),
        }
    }

    fn line() -> String {
        format!("{DIGEST} {}\n", founder().member)
    }

    /// How the fakes answer: whether `mandate` writes its file, the ConfigMap's earlier content
    /// (`None`: there is none), whether the api Deployment exists, and whether apply and restart
    /// succeed.
    struct World {
        links_ok: bool,
        before: Option<String>,
        api: bool,
        applies: bool,
        restarts: bool,
    }

    impl Default for World {
        fn default() -> Self {
            Self {
                links_ok: true,
                before: None,
                api: true,
                applies: true,
                restarts: true,
            }
        }
    }

    /// What the fakes saw: each `mandate` call's arguments and standard input, and each `kubectl`
    /// call's arguments and standard input.
    #[derive(Default)]
    struct Seen {
        mandate: Vec<(Vec<String>, String)>,
        kubectl: Vec<(Vec<String>, Option<String>)>,
    }

    impl Seen {
        fn verbs(&self) -> Vec<String> {
            self.kubectl
                .iter()
                .map(|(args, _)| args[..2].join(" "))
                .collect()
        }

        fn applied(&self) -> Option<serde_json::Value> {
            self.kubectl
                .iter()
                .find(|(args, _)| args[0] == "apply")
                .and_then(|(_, stdin)| stdin.as_deref())
                .map(|text| serde_json::from_str(text).expect("JSON"))
        }
    }

    static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn scratch() -> PathBuf {
        let n = RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("xtask-founding-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    fn run(subject: &str, world: &World) -> (Result<String>, Seen, PathBuf) {
        let seen = RefCell::new(Seen::default());
        let dir = scratch();
        let mandate = |args: &[String], stdin: &str| -> Result<bool> {
            seen.borrow_mut()
                .mandate
                .push((args.to_vec(), stdin.to_owned()));
            let links = args
                .iter()
                .position(|a| a == "--links")
                .and_then(|i| args.get(i + 1));
            match (world.links_ok, links) {
                (true, Some(path)) => {
                    std::fs::write(path, line())?;
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
            Ok(match args[..2] {
                ["get", "configmap"] => match &world.before {
                    Some(text) => (
                        true,
                        serde_json::json!({"data": {CONFIGMAP: text}}).to_string(),
                    ),
                    None => (false, String::new()),
                },
                ["get", "deployment"] => (world.api, "deployment.apps/api".to_owned()),
                ["apply", _] => (world.applies, String::new()),
                ["rollout", "restart"] => (world.restarts, String::new()),
                _ => (false, String::new()),
            })
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
        let (got, seen, dir) = run(CANARY, &World::default());
        assert_eq!(got.expect("linked"), "linked; api restarted");
        assert_eq!(seen.mandate.len(), 1);
        let (args, stdin) = &seen.mandate[0];
        assert_eq!(stdin, CANARY, "the subject is mandate's standard input");
        assert!(!args.iter().any(|a| a.contains(CANARY)), "{args:?}");
        assert_eq!(&args[..6], &link_command(&founder(), Path::new("x"))[..6]);
        for (args, stdin) in &seen.kubectl {
            assert!(
                !args
                    .iter()
                    .any(|a| a.contains(CANARY) || a.contains(DIGEST)),
                "{args:?}"
            );
            assert!(!stdin.as_deref().unwrap_or_default().contains(CANARY));
        }
        let applied = seen.applied().expect("applied");
        assert_eq!(applied["kind"], "ConfigMap");
        assert_eq!(applied["metadata"]["name"], CONFIGMAP);
        assert_eq!(applied["metadata"]["namespace"], "ws-ws1");
        assert_eq!(applied["data"][CONFIGMAP], line());
        assert_eq!(
            seen.verbs(),
            [
                "get configmap",
                "apply -f",
                "get deployment",
                "rollout restart"
            ]
        );
        assert!(walk(&dir).is_empty(), "the link file is removed");
    }

    #[test]
    fn the_api_restarts_only_when_the_link_changed_and_it_exists() {
        let same = World {
            before: Some(line()),
            ..World::default()
        };
        let (got, seen, _) = run(CANARY, &same);
        assert_eq!(got.expect("unchanged"), "linked, unchanged");
        assert_eq!(seen.verbs(), ["get configmap", "apply -f"]);
        let other = World {
            before: Some("sha256:00 01KDEM0000000000000000000P\n".to_owned()),
            ..World::default()
        };
        let (got, seen, _) = run(CANARY, &other);
        assert_eq!(got.expect("restarted"), "linked; api restarted");
        assert!(seen.verbs().contains(&"rollout restart".to_owned()));
        let no_api = World {
            api: false,
            ..World::default()
        };
        let (got, seen, _) = run(CANARY, &no_api);
        assert_eq!(got.expect("linked"), "linked");
        assert!(!seen.verbs().contains(&"rollout restart".to_owned()));
        let stuck = World {
            restarts: false,
            ..World::default()
        };
        assert!(
            run(CANARY, &stuck).0.is_err(),
            "a failed restart fails the step"
        );
    }

    #[test]
    fn no_subject_applies_the_links_empty_and_runs_no_mandate() {
        let (got, seen, _) = run("", &World::default());
        assert_eq!(
            got.expect("cleared"),
            "no founder subject is set: nobody is linked; api restarted"
        );
        assert!(seen.mandate.is_empty());
        assert_eq!(seen.applied().expect("applied")["data"][CONFIGMAP], "");
        let empty = World {
            before: Some(String::new()),
            ..World::default()
        };
        let (got, seen, _) = run("", &empty);
        assert_eq!(
            got.expect("unchanged"),
            "no founder subject is set: nobody is linked, unchanged"
        );
        assert_eq!(seen.verbs(), ["get configmap", "apply -f"]);
    }

    #[test]
    fn a_refused_link_applies_nothing_and_a_failed_apply_fails_the_step() {
        let refused = World {
            links_ok: false,
            ..World::default()
        };
        let (got, seen, dir) = run(CANARY, &refused);
        let error = got.expect_err("refused").to_string();
        assert!(!error.contains(CANARY), "{error}");
        assert!(seen.kubectl.is_empty(), "nothing read or applied");
        assert!(walk(&dir).is_empty());
        let failing = World {
            applies: false,
            ..World::default()
        };
        let (got, seen, _) = run(CANARY, &failing);
        assert!(got.is_err());
        assert_eq!(
            seen.verbs(),
            ["get configmap", "apply -f"],
            "no restart after a failed apply"
        );
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

    #[test]
    fn the_real_run_mandate_clears_the_environment_and_discards_the_output() {
        let dir = scratch();
        let seen = dir.join("seen");
        let stub = dir.join("mandate");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\n\
                 env > '{seen}.env'\n\
                 printf '%s\\n' \"$@\" > '{seen}.args'\n\
                 cat > '{seen}.stdin'\n\
                 out=$(readlink /proc/$$/fd/1)\n\
                 err=$(readlink /proc/$$/fd/2)\n\
                 echo \"$out\" > '{seen}.out'\n\
                 echo \"$err\" > '{seen}.err'\n\
                 echo {CANARY}\n\
                 exit \"$1\"\n",
                seen = seen.display()
            ),
        )
        .expect("the stub");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).expect("mode");
        let read = |ext: &str| {
            std::fs::read_to_string(format!("{}.{ext}", seen.display())).expect("recorded")
        };
        assert!(run_mandate(&stub, &["0".to_owned(), "link".to_owned()], CANARY).expect("ran"));
        assert_eq!(read("stdin"), CANARY, "the subject is its standard input");
        assert_eq!(read("args"), "0\nlink\n", "and no argument");
        let env = read("env");
        for inherited in ["PATH=", "HOME="] {
            assert!(
                !env.contains(inherited),
                "the environment is cleared: {env}"
            );
        }
        assert_eq!(read("out").trim(), "/dev/null", "its output is discarded");
        assert_eq!(read("err").trim(), "/dev/null", "its errors are discarded");
        assert!(
            !run_mandate(&stub, &["3".to_owned()], CANARY).expect("ran"),
            "a refusal"
        );
    }

    #[test]
    fn a_child_that_inherits_the_step_s_environment_drops_its_secrets() {
        let mut command = Command::new("tofu");
        let envs: Vec<_> = without_step_secrets(&mut command)
            .get_envs()
            .map(|(name, value)| (name.to_owned(), value.map(ToOwned::to_owned)))
            .collect();
        for secret in STEP_SECRETS {
            assert!(
                envs.contains(&(secret.into(), None)),
                "{secret} is removed: {envs:?}"
            );
        }
        assert!(STEP_SECRETS.contains(&super::SUBJECT_VAR));
    }

    /// Every child the infra step starts either begins from an empty environment or drops the
    /// step's secrets: a `Command::new(` with neither close by is refused.
    #[test]
    fn every_child_of_the_infra_step_starts_without_the_step_s_secrets() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let read = |file: &str| std::fs::read_to_string(src.join(file)).expect("source");
        let main = read("main.rs");
        let mut bodies: Vec<(String, String)> = [
            "infra_preflight.rs",
            "infra_window.rs",
            "infra_verify.rs",
            "vault_init.rs",
            "vault_provision.rs",
            "gitops.rs",
            "founding.rs",
        ]
        .into_iter()
        .map(|file| {
            let text = read(file);
            let code = text
                .split("#[cfg(test)]\nmod tests")
                .next()
                .unwrap_or_default();
            (file.to_owned(), code.to_owned())
        })
        .collect();
        for function in [
            "\nfn head_commit(",
            "\nfn run_tofu(",
            "\nfn changed_since_applied(",
            "\nfn bootstrap_staging(",
        ] {
            let start = main.find(function).expect(function);
            let end = main[start + 1..].find("\n}\n").expect("its end") + start + 3;
            bodies.push((function.trim().to_owned(), main[start..end].to_owned()));
        }
        let mut problems = Vec::new();
        for (name, code) in &bodies {
            for (at, _) in code.match_indices("Command::new(") {
                let before = &code[at.saturating_sub(40)..at];
                let near = &code[at..(at + 400).min(code.len())];
                if !(before.contains("without_step_secrets(&mut ")
                    || near.contains("env_clear()")
                    || near.contains("without_step_secrets(&mut command)"))
                {
                    problems.push(format!("{name}: {}", &code[at..(at + 60).min(code.len())]));
                }
            }
        }
        assert!(
            problems.is_empty(),
            "children inheriting the step's secrets: {problems:#?}"
        );
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
