//! The `admission` drill (E21-58, DEC-2016 item 2): the hosted clusters' kill-switch admission
//! policies, `deploy/gitops/base/kill-switch-admission.yaml`, applied as they are to the cell's
//! API server, with a signature webhook that is down and fails closed, skipping exactly the pods
//! `deploy/gitops/base/admission.yaml`'s post-render patch exempts, as the hosted clusters' does
//! when policy-controller is unavailable.
//!
//! In namespaces of their own, labelled as the workspace's is, it checks that the founder's
//! kill-switch Job is admitted and its pod made, with the webhook down; that every other way to a
//! pod labelled `mandate.dev/process: control` is refused (a person's pod, a service account's
//! pod or Job, a service account relabelling its suspended Job, a Deployment's pod, a CronJob's
//! Job, the label added to a pod later); that an unlabelled pod is refused while the webhook is
//! down; that a second workspace's namespace is covered by its label alone; and that a finished
//! kill-switch Job is cleaned up by foreground deletion. It waits on conditions, not clocks, and
//! leaves the cell as it found it.

use std::fs;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};

use crate::cell::{NODE, Verdict};

/// The policies the drill applies: the file the hosted clusters' base applies.
const POLICIES: &str = "deploy/gitops/base/kill-switch-admission.yaml";

/// The file whose post-render patch gives the signature webhooks their exemption.
const ADMISSION: &str = "deploy/gitops/base/admission.yaml";

/// What the seeded drill changes: the Job policy matches creation only, so a service account can
/// relabel its suspended Job's pods `control` (the review of #1810, B1).
const JOB_OPERATIONS: &str = "        operations: [CREATE, UPDATE]\n        resources: [jobs]";
const JOB_OPERATIONS_SEEDED: &str = "        operations: [CREATE]\n        resources: [jobs]";

/// The namespaces the drill makes, each labelled as a workspace's namespace is.
const NAMESPACES: [&str; 2] = ["admission-drill-1", "admission-drill-2"];

/// The label that puts a namespace under the signature webhooks and these policies.
const INCLUDE: &str = "policy.sigstore.dev/include=true";

/// The down webhook's name, the drill's own so it never meets a real one.
const WEBHOOK: &str = "admission-drill.policy.sigstore.dev";

/// A person, as the founder's CLI is, and a workload's service account, as the deployment
/// manager's is; both may do anything RBAC allows, so only admission can refuse them.
const FOUNDER: [&str; 2] = ["--as=founder", "--as-group=system:masters"];
const WORKLOAD: [&str; 2] = [
    "--as=system:serviceaccount:admission-drill-1:deploy-manager",
    "--as-group=system:masters",
];

/// A pinned image of this repository's; the drill never needs it pulled, only admitted.
const IMAGE: &str = "ghcr.io/kunwarshivam/mandate/mandate@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// How many one-second polls a wait takes before it gives up.
const POLLS: u32 = 60;

/// Runs kubectl with `args` and `stdin`, as `(succeeded, stdout and stderr)`.
pub(crate) type Kubectl<'a> = &'a dyn Fn(&[&str], Option<&str>) -> Result<(bool, String)>;

/// kubectl in the cell's k3s node, as the cell's other drills run it.
pub(crate) fn node_kubectl(args: &[&str], stdin: Option<&str>) -> Result<(bool, String)> {
    let mut full = vec!["exec", "-i", NODE, "kubectl"];
    full.extend_from_slice(args);
    let mut child = Command::new("docker")
        .args(&full)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting kubectl in the cell")?;
    child
        .stdin
        .take()
        .context("kubectl's stdin")?
        .write_all(stdin.unwrap_or_default().as_bytes())?;
    let out = child.wait_with_output()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), text))
}

/// What admission made of one request.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Admitted,
    RefusedByPolicy,
    RefusedByWebhook,
    Other(String),
}

fn outcome((ok, text): (bool, String)) -> Outcome {
    if ok {
        Outcome::Admitted
    } else if text.contains("ValidatingAdmissionPolicy") {
        Outcome::RefusedByPolicy
    } else if text.contains("failed calling webhook") {
        Outcome::RefusedByWebhook
    } else {
        Outcome::Other(text.trim().to_owned())
    }
}

/// The `objectSelector` value `admission.yaml`'s patch gives the validating webhook, as YAML lines
/// indented under `indent`: the drill's down webhook skips exactly what the hosted one does.
fn exemption_selector(admission: &str, indent: &str) -> Result<String> {
    let target = admission
        .find("kind: ValidatingWebhookConfiguration")
        .context("admission.yaml patches no ValidatingWebhookConfiguration")?;
    let rest = &admission[target..];
    let start = rest
        .find("path: /webhooks/0/objectSelector")
        .context("the validating webhook's patch sets no objectSelector")?;
    let mut lines = rest[start..].lines().skip(1);
    let value = lines.next().context("the patch has no value")?;
    ensure!(
        value.trim() == "value:",
        "the patch's value is not a block: {value:?}"
    );
    let depth = value.len() - value.trim_start().len();
    let block: Vec<&str> = lines
        .take_while(|line| line.len() - line.trim_start().len() > depth && !line.trim().is_empty())
        .collect();
    let base = block
        .iter()
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .context("the patch's value is empty")?;
    Ok(block
        .iter()
        .map(|line| format!("{indent}{}\n", &line[base..]))
        .collect())
}

/// The signature webhook, down: its service does not exist, it fails closed, and it skips what
/// `selector` does.
fn down_webhook(selector: &str) -> String {
    format!(
        "apiVersion: admissionregistration.k8s.io/v1\nkind: ValidatingWebhookConfiguration\n\
         metadata: {{name: {WEBHOOK}}}\nwebhooks:\n  - name: {WEBHOOK}\n    \
         admissionReviewVersions: [v1]\n    sideEffects: None\n    failurePolicy: Fail\n    \
         timeoutSeconds: 5\n    namespaceSelector: {{matchExpressions: [{{key: \
         policy.sigstore.dev/include, operator: In, values: [\"true\"]}}]}}\n    objectSelector:\n\
         {selector}    clientConfig: {{service: {{name: absent, namespace: admission-drill-1, path: \
         /validations}}}}\n    rules: [{{apiGroups: [\"\"], apiVersions: [v1], operations: [CREATE, \
         UPDATE], resources: [pods]}}]\n"
    )
}

fn pod(name: &str, process: &str) -> String {
    format!(
        "apiVersion: v1\nkind: Pod\nmetadata: {{name: {name}, labels: {{mandate.dev/process: {process}}}}}\n\
         spec: {{containers: [{{name: c, image: \"{IMAGE}\"}}]}}\n"
    )
}

fn job(name: &str, process: &str, suspend: bool) -> String {
    format!(
        "apiVersion: batch/v1\nkind: Job\nmetadata: {{name: {name}}}\nspec:\n  suspend: {suspend}\n  \
         backoffLimit: 0\n  template:\n    metadata: {{labels: {{mandate.dev/process: {process}}}}}\n    \
         spec: {{restartPolicy: Never, containers: [{{name: c, image: \"{IMAGE}\"}}]}}\n"
    )
}

/// Notes in `broken` that `what` got `got` rather than `want`.
fn expect(broken: &mut Vec<String>, what: &str, got: Outcome, want: Outcome) {
    if got != want {
        broken.push(format!("{what}: {got:?}, not {want:?}"));
    }
}

/// Polls `check` once a second until it holds, up to [`POLLS`] times.
fn wait_for(check: &mut dyn FnMut() -> Result<bool>, pause: Duration) -> Result<bool> {
    for _ in 0..POLLS {
        if check()? {
            return Ok(true);
        }
        sleep(pause);
    }
    Ok(false)
}

/// The drill's run against `kubectl`, with `policies` as the admission policies, `selector` as
/// the webhook's exemption, and `pause` between polls.
pub(crate) fn run(
    kubectl: Kubectl<'_>,
    policies: &str,
    selector: &str,
    pause: Duration,
) -> Result<Verdict> {
    let as_who = |who: &[&str], rest: &[&str]| -> Vec<String> {
        who.iter()
            .chain(rest)
            .map(|arg| (*arg).to_owned())
            .collect()
    };
    let call = |args: Vec<String>, stdin: Option<&str>| -> Result<(bool, String)> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        kubectl(&args, stdin)
    };
    let create = |who: &[&str], namespace: &str, manifest: &str| -> Result<Outcome> {
        Ok(outcome(call(
            as_who(who, &["create", "--namespace", namespace, "-f", "-"]),
            Some(manifest),
        )?))
    };
    let pods = |namespace: &str, selector: &str| -> Result<usize> {
        let (_, listed) = kubectl(
            &[
                "get",
                "pods",
                "--namespace",
                namespace,
                "--selector",
                selector,
                "-o",
                "name",
            ],
            None,
        )?;
        Ok(listed
            .lines()
            .filter(|line| line.starts_with("pod/"))
            .count())
    };
    let (main, other) = (NAMESPACES[0], NAMESPACES[1]);
    for namespace in NAMESPACES {
        let (gone, text) = kubectl(
            &[
                "delete",
                "namespace",
                namespace,
                "--ignore-not-found",
                "--wait=true",
                "--timeout=180s",
            ],
            None,
        )?;
        ensure!(
            gone,
            "a previous run's {namespace} is still terminating: {}",
            text.trim()
        );
        let (made, text) = kubectl(&["create", "namespace", namespace], None)?;
        ensure!(made, "creating {namespace} failed: {}", text.trim());
        let (labelled, text) = kubectl(&["label", "namespace", namespace, INCLUDE], None)?;
        ensure!(labelled, "labelling {namespace} failed: {}", text.trim());
    }
    let (applied, text) = kubectl(&["apply", "-f", "-"], Some(policies))?;
    ensure!(applied, "applying the admission policies failed: {text}");
    let (applied, text) = kubectl(&["apply", "-f", "-"], Some(&down_webhook(selector)))?;
    ensure!(applied, "applying the down webhook failed: {text}");
    let mut probes = 0;
    let ready = wait_for(
        &mut || {
            probes += 1;
            Ok(
                create(&FOUNDER, main, &pod(&format!("probe-{probes}"), "control"))?
                    == Outcome::RefusedByPolicy
                    && create(&FOUNDER, main, &pod(&format!("probe-api-{probes}"), "api"))?
                        == Outcome::RefusedByWebhook,
            )
        },
        pause,
    )?;
    ensure!(
        ready,
        "the admission policies and the down webhook never took effect"
    );

    let mut broken = Vec::new();
    expect(
        &mut broken,
        "the founder's kill-switch Job",
        create(&FOUNDER, main, &job("kill-switch", "control", false))?,
        Outcome::Admitted,
    );
    if !wait_for(&mut || Ok(pods(main, "job-name=kill-switch")? > 0), pause)? {
        broken.push("the kill switch's pod was not made with the webhook down".to_owned());
    }
    expect(
        &mut broken,
        "a service account's pod labelled control",
        create(&WORKLOAD, main, &pod("workload", "control"))?,
        Outcome::RefusedByPolicy,
    );
    expect(
        &mut broken,
        "a service account's control Job",
        create(&WORKLOAD, main, &job("workload-job", "control", false))?,
        Outcome::RefusedByPolicy,
    );
    expect(
        &mut broken,
        "a labelled pod in a second workspace's namespace",
        create(&FOUNDER, other, &pod("second", "control"))?,
        Outcome::RefusedByPolicy,
    );
    expect(
        &mut broken,
        "a service account's suspended Job, unlabelled",
        create(&WORKLOAD, main, &job("suspended", "api", true))?,
        Outcome::Admitted,
    );
    let relabel =
        r#"{"spec":{"template":{"metadata":{"labels":{"mandate.dev/process":"control"}}}}}"#;
    if outcome(call(
        as_who(
            &WORKLOAD,
            &[
                "patch",
                "job",
                "suspended",
                "--type=merge",
                "-p",
                relabel,
                "--namespace",
                main,
            ],
        ),
        None,
    )?) != Outcome::RefusedByPolicy
    {
        broken.push("admitted the control label on a suspended Job".to_owned());
    }
    let deployment = format!(
        "apiVersion: apps/v1\nkind: Deployment\nmetadata: {{name: fake-control}}\nspec:\n  replicas: 1\n  \
         selector: {{matchLabels: {{mandate.dev/process: control, app: fake-control}}}}\n  template:\n    \
         metadata: {{labels: {{mandate.dev/process: control, app: fake-control}}}}\n    spec: {{containers: \
         [{{name: c, image: \"{IMAGE}\"}}]}}\n"
    );
    expect(
        &mut broken,
        "the labelled Deployment itself",
        create(&FOUNDER, main, &deployment)?,
        Outcome::Admitted,
    );
    let failed = wait_for(
        &mut || {
            let (_, status) = kubectl(
                &[
                    "get",
                    "replicasets",
                    "--namespace",
                    main,
                    "--selector",
                    "app=fake-control",
                    "-o",
                    "jsonpath={.items[*].status.conditions[?(@.type==\"ReplicaFailure\")].status}",
                ],
                None,
            )?;
            Ok(status.trim() == "True")
        },
        pause,
    )?;
    if !failed || pods(main, "app=fake-control")? > 0 {
        broken.push("admitted a labelled Deployment's pod".to_owned());
    }
    let cronjob = format!(
        "apiVersion: batch/v1\nkind: CronJob\nmetadata: {{name: fake-control}}\nspec:\n  schedule: \"0 0 1 1 *\"\n  \
         jobTemplate:\n    spec:\n      template:\n        metadata: {{labels: {{mandate.dev/process: control}}}}\n        \
         spec: {{restartPolicy: Never, containers: [{{name: c, image: \"{IMAGE}\"}}]}}\n"
    );
    expect(
        &mut broken,
        "the labelled CronJob itself",
        create(&FOUNDER, main, &cronjob)?,
        Outcome::Admitted,
    );
    expect(
        &mut broken,
        "a Job a CronJob owns",
        outcome(call(
            as_who(
                &FOUNDER,
                &[
                    "create",
                    "job",
                    "from-cronjob",
                    "--from=cronjob/fake-control",
                    "--namespace",
                    main,
                ],
            ),
            None,
        )?),
        Outcome::RefusedByPolicy,
    );
    let (removed, text) = kubectl(&["delete", "validatingwebhookconfiguration", WEBHOOK], None)?;
    ensure!(removed, "removing the down webhook failed: {}", text.trim());
    let admitted = wait_for(
        &mut || Ok(create(&FOUNDER, main, &pod("relabelled", "api"))? == Outcome::Admitted),
        pause,
    )?;
    ensure!(
        admitted,
        "an unlabelled pod was never admitted once the webhook was gone"
    );
    expect(
        &mut broken,
        "the control label added to a pod",
        outcome(call(
            as_who(
                &FOUNDER,
                &[
                    "label",
                    "pod",
                    "relabelled",
                    "mandate.dev/process=control",
                    "--overwrite",
                    "--namespace",
                    main,
                ],
            ),
            None,
        )?),
        Outcome::RefusedByPolicy,
    );
    let (deleted, text) = kubectl(
        &[
            "delete",
            "job",
            "kill-switch",
            "--cascade=foreground",
            "--timeout=120s",
            "--namespace",
            main,
        ],
        None,
    )?;
    if !deleted {
        broken.push(format!(
            "the kill-switch Job's foreground deletion failed: {}",
            text.trim()
        ));
    }

    kubectl(&["delete", "-f", "-", "--ignore-not-found"], Some(policies))?;
    for namespace in NAMESPACES {
        kubectl(&["delete", "namespace", namespace, "--wait=false"], None)?;
    }
    Ok(if broken.is_empty() {
        Verdict::Held
    } else {
        Verdict::Broken(broken.join("; "))
    })
}

/// The drill in the cell; seeded, the Job policy matches creation only.
pub(crate) fn drill(seeded: bool) -> Result<Verdict> {
    let policies = fs::read_to_string(POLICIES).with_context(|| format!("reading {POLICIES}"))?;
    let admission =
        fs::read_to_string(ADMISSION).with_context(|| format!("reading {ADMISSION}"))?;
    let selector = exemption_selector(&admission, "      ")?;
    let policies = if seeded {
        if !policies.contains(JOB_OPERATIONS) {
            bail!("{POLICIES} no longer holds the Job operations the seeded drill narrows");
        }
        policies.replace(JOB_OPERATIONS, JOB_OPERATIONS_SEEDED)
    } else {
        policies
    };
    run(&node_kubectl, &policies, &selector, Duration::from_secs(1))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;

    use super::*;

    fn repo_file(path: &str) -> Result<String> {
        Ok(fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path),
        )?)
    }

    /// The down webhook skips exactly what `admission.yaml`'s patch exempts.
    #[test]
    fn the_exemption_is_read_from_the_hosted_patch() -> Result<()> {
        let selector = exemption_selector(&repo_file(ADMISSION)?, "      ")?;
        assert_eq!(
            selector,
            "      matchExpressions:\n        - key: mandate.dev/process\n          operator: NotIn\n          \
             values: [control]\n"
        );
        assert!(down_webhook(&selector).contains("    objectSelector:\n      matchExpressions:\n"));
        assert!(exemption_selector("kind: MutatingWebhookConfiguration\n", "  ").is_err());
        Ok(())
    }

    /// The seeded drill narrows the Job policy the file holds, and nothing else.
    #[test]
    fn the_seeded_change_is_in_the_job_policy() -> Result<()> {
        let policies = repo_file(POLICIES)?;
        assert_eq!(policies.matches(JOB_OPERATIONS).count(), 1);
        let seeded = policies.replace(JOB_OPERATIONS, JOB_OPERATIONS_SEEDED);
        assert_eq!(
            seeded.matches("operations: [CREATE, UPDATE]").count(),
            1,
            "the pod policy keeps UPDATE"
        );
        Ok(())
    }

    #[test]
    fn outcomes_are_read_from_kubectls_answer() {
        assert_eq!(outcome((true, String::new())), Outcome::Admitted);
        assert_eq!(
            outcome((
                false,
                "ValidatingAdmissionPolicy 'mandate-control-pods' denied".to_owned()
            )),
            Outcome::RefusedByPolicy
        );
        assert_eq!(
            outcome((false, "Internal error: failed calling webhook".to_owned())),
            Outcome::RefusedByWebhook
        );
        assert_eq!(
            outcome((false, "boom".to_owned())),
            Outcome::Other("boom".to_owned())
        );
    }

    /// A cluster that admits as the hosted policies should, or with one of the drill's seeded or
    /// imagined holes: `relabel` admits a service account's suspended-Job relabel, `deployment`
    /// admits a labelled Deployment's pod.
    struct Fake {
        relabel: bool,
        deployment: bool,
        webhook: RefCell<bool>,
    }

    const POLICY: &str = "ValidatingAdmissionPolicy 'mandate-control' denied request";

    impl Fake {
        fn answer(&self, args: &[&str], stdin: Option<&str>) -> Result<(bool, String)> {
            let manifest = stdin.unwrap_or_default();
            let workload = args.iter().any(|arg| arg.contains("serviceaccount"));
            let control = manifest.contains("mandate.dev/process: control");
            let refuse = |ok: bool| -> Result<(bool, String)> {
                Ok(if ok {
                    (true, String::new())
                } else {
                    (false, POLICY.to_owned())
                })
            };
            match args.iter().find(|arg| !arg.starts_with("--")).copied() {
                Some("apply") if manifest.contains(WEBHOOK) => {
                    self.webhook.replace(true);
                    Ok((true, String::new()))
                }
                Some("delete") if args.contains(&"validatingwebhookconfiguration") => {
                    self.webhook.replace(false);
                    Ok((true, String::new()))
                }
                Some("create") if manifest.contains("kind: Pod") => {
                    if control {
                        refuse(false)
                    } else if *self.webhook.borrow() {
                        Ok((false, "failed calling webhook".to_owned()))
                    } else {
                        refuse(true)
                    }
                }
                Some("create") if manifest.contains("kind: Job") => refuse(!(control && workload)),
                Some("create") if args.contains(&"--from=cronjob/fake-control") => refuse(false),
                Some("patch") => refuse(self.relabel || !workload),
                Some("label") if args.contains(&"mandate.dev/process=control") => refuse(false),
                Some("get") if args.contains(&"job-name=kill-switch") => {
                    Ok((true, "pod/kill-switch-x\n".to_owned()))
                }
                Some("get") if args.contains(&"replicasets") => Ok((true, "True".to_owned())),
                Some("get") if args.contains(&"app=fake-control") => Ok((
                    true,
                    if self.deployment {
                        "pod/fake-control-x\n"
                    } else {
                        ""
                    }
                    .to_owned(),
                )),
                _ => Ok((true, String::new())),
            }
        }
    }

    fn drill_against(fake: &Fake) -> Result<Verdict> {
        let policies = repo_file(POLICIES)?;
        let selector = exemption_selector(&repo_file(ADMISSION)?, "      ")?;
        run(
            &|args, stdin| fake.answer(args, stdin),
            &policies,
            &selector,
            Duration::ZERO,
        )
    }

    /// The drill holds against a cluster that admits as the policies should, and names each hole
    /// it is meant to find: the seeded suspended-Job relabel and an admitted Deployment's pod.
    #[test]
    fn the_drill_holds_only_where_the_policies_hold() -> Result<()> {
        let good = Fake {
            relabel: false,
            deployment: false,
            webhook: RefCell::new(false),
        };
        assert_eq!(drill_against(&good)?, Verdict::Held);
        let relabel = Fake {
            relabel: true,
            deployment: false,
            webhook: RefCell::new(false),
        };
        assert!(matches!(
            drill_against(&relabel)?,
            Verdict::Broken(why) if why == "admitted the control label on a suspended Job"
        ));
        let deployment = Fake {
            relabel: false,
            deployment: true,
            webhook: RefCell::new(false),
        };
        assert!(matches!(
            drill_against(&deployment)?,
            Verdict::Broken(why) if why == "admitted a labelled Deployment's pod"
        ));
        Ok(())
    }
}
