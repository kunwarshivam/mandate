//! The `admission` drill (E21-58, DEC-2016 item 2): the hosted clusters' kill-switch admission
//! policies, `deploy/gitops/base/kill-switch-admission.yaml`, applied as they are to the cell's
//! API server, with a signature webhook that is down and fails closed, skipping exactly the pods
//! `deploy/gitops/base/admission.yaml`'s post-render patch exempts, as the hosted clusters' does
//! when policy-controller is unavailable.
//!
//! In namespaces of their own, labelled as the workspace's is, it checks that the founder's
//! kill-switch Job is admitted and its pod made, with the webhook down; that every other way to a
//! pod labelled `mandate.dev/process: control` is refused (a person's pod, a service account's
//! pod or Job, a service account relabelling its suspended Job or changing a person's, a
//! Deployment's pod, a CronJob's Job, the label added to a pod later, and a changed image or an
//! ephemeral container in the kill switch's pod); that an unlabelled pod is refused while the
//! webhook is down; that a second workspace's namespace is covered by its label alone; and that a
//! finished kill-switch Job is cleaned up by foreground deletion. It waits on conditions, not
//! clocks, and leaves the cell as it found it, whether a step errs or not.

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

/// What the seeded drill changes: on update, the Job policy fires only when the control label is
/// new, so a service account can change the pods of a control Job a person made (the review of
/// #1810's second round, N1).
const JOB_CHANGED: &str = "    - name: changed-control\n      \
                           expression: request.operation == 'CREATE' || object.spec != oldObject.spec\n";
const JOB_CHANGED_SEEDED: &str = "    - name: changed-control\n      expression: >-\n        \
                                  request.operation == 'CREATE' || !(has(oldObject.spec.template.metadata.labels)\n        \
                                  && 'mandate.dev/process' in oldObject.spec.template.metadata.labels\n        \
                                  && oldObject.spec.template.metadata.labels['mandate.dev/process'] == 'control')\n";

/// An image other than [`IMAGE`], that a change to a control pod or Job swaps in.
const OTHER_IMAGE: &str =
    "ghcr.io/example/other@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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
/// the webhook's exemption, and `pause` between polls. What it made is removed whether or not a
/// step errs.
pub(crate) fn run(
    kubectl: Kubectl<'_>,
    policies: &str,
    selector: &str,
    pause: Duration,
) -> Result<Verdict> {
    clean_up(kubectl, policies).context("a previous run's leftovers would not go")?;
    let exercised = exercise(kubectl, policies, selector, pause);
    let cleaned = clean_up(kubectl, policies);
    let broken = exercised.map_err(|error| match &cleaned {
        Ok(()) => error,
        Err(also) => error.context(format!("and cleaning up after it failed: {also:#}")),
    })?;
    cleaned?;
    Ok(if broken.is_empty() {
        Verdict::Held
    } else {
        Verdict::Broken(broken.join("; "))
    })
}

/// Removes the down webhook, the policies and the namespaces, waiting until the namespaces are
/// gone, as the start of a run does.
fn clean_up(kubectl: Kubectl<'_>, policies: &str) -> Result<()> {
    let (gone, text) = kubectl(
        &[
            "delete",
            "validatingwebhookconfiguration",
            WEBHOOK,
            "--ignore-not-found",
        ],
        None,
    )?;
    ensure!(gone, "removing the down webhook failed: {}", text.trim());
    let (gone, text) = kubectl(&["delete", "-f", "-", "--ignore-not-found"], Some(policies))?;
    ensure!(
        gone,
        "removing the admission policies failed: {}",
        text.trim()
    );
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
        ensure!(gone, "{namespace} is still terminating: {}", text.trim());
    }
    Ok(())
}

/// The drill's scenarios, as what each found broken.
fn exercise(
    kubectl: Kubectl<'_>,
    policies: &str,
    selector: &str,
    pause: Duration,
) -> Result<Vec<String>> {
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
    if wait_for(&mut || Ok(pods(main, "job-name=kill-switch")? > 0), pause)? {
        let (_, listed) = kubectl(
            &[
                "get",
                "pods",
                "--namespace",
                main,
                "--selector",
                "job-name=kill-switch",
                "-o",
                "name",
            ],
            None,
        )?;
        let made = listed
            .lines()
            .find(|line| line.starts_with("pod/"))
            .context("the kill switch's pod went before it was changed")?;
        let swapped = format!("c={OTHER_IMAGE}");
        expect(
            &mut broken,
            "a service account's changed image in the kill switch's pod",
            outcome(call(
                as_who(
                    &WORKLOAD,
                    &["set", "image", made, &swapped, "--namespace", main],
                ),
                None,
            )?),
            Outcome::RefusedByPolicy,
        );
        expect(
            &mut broken,
            "a service account's ephemeral container in the kill switch's pod",
            outcome(call(
                as_who(
                    &WORKLOAD,
                    &[
                        "debug",
                        made,
                        "--image",
                        OTHER_IMAGE,
                        "--container",
                        "debug",
                        "--namespace",
                        main,
                    ],
                ),
                None,
            )?),
            Outcome::RefusedByPolicy,
        );
    } else {
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
    expect(
        &mut broken,
        "a person's suspended control Job",
        create(&FOUNDER, main, &job("held", "control", true))?,
        Outcome::Admitted,
    );
    let swap = r#"{"spec":{"template":{"spec":{"nodeSelector":{"admission-drill":"moved"}}}}}"#;
    if outcome(call(
        as_who(
            &WORKLOAD,
            &[
                "patch",
                "job",
                "held",
                "--type=merge",
                "-p",
                swap,
                "--namespace",
                main,
            ],
        ),
        None,
    )?) != Outcome::RefusedByPolicy
    {
        broken.push("admitted a service account's change to a person's control Job".to_owned());
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

    Ok(broken)
}

/// The drill in the cell; seeded, the Job policy on update fires only on a newly control label.
pub(crate) fn drill(seeded: bool) -> Result<Verdict> {
    let policies = fs::read_to_string(POLICIES).with_context(|| format!("reading {POLICIES}"))?;
    let admission =
        fs::read_to_string(ADMISSION).with_context(|| format!("reading {ADMISSION}"))?;
    let selector = exemption_selector(&admission, "      ")?;
    let policies = if seeded {
        if !policies.contains(JOB_CHANGED) {
            bail!("{POLICIES} no longer holds the Job condition the seeded drill narrows");
        }
        policies.replace(JOB_CHANGED, JOB_CHANGED_SEEDED)
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
        assert_eq!(policies.matches(JOB_CHANGED).count(), 1);
        let seeded = policies.replace(JOB_CHANGED, JOB_CHANGED_SEEDED);
        assert_eq!(
            seeded.matches("object.spec != oldObject.spec").count(),
            1,
            "the pod policy keeps its spec check"
        );
        assert!(seeded.contains("resources: [jobs]\n  matchConditions:"));
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
    /// imagined holes: `relabel` admits a service account's suspended-Job relabel, `change` its
    /// change to a person's control Job, `deployment` a labelled Deployment's pod. `fails` names
    /// a step that errs, and `deleted` records what the drill removed.
    #[derive(Default)]
    struct Fake {
        relabel: bool,
        change: bool,
        deployment: bool,
        fails: Option<&'static str>,
        webhook: RefCell<bool>,
        deleted: RefCell<Vec<String>>,
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
            if let Some(step) = self.fails
                && args.contains(&step)
            {
                bail!("the cluster went away during {step}");
            }
            match args.iter().find(|arg| !arg.starts_with("--")).copied() {
                Some("apply") if manifest.contains(WEBHOOK) => {
                    self.webhook.replace(true);
                    Ok((true, String::new()))
                }
                Some("delete") => {
                    if args.contains(&"validatingwebhookconfiguration") {
                        self.webhook.replace(false);
                    }
                    self.deleted.borrow_mut().push(
                        args.iter()
                            .filter(|arg| !arg.starts_with("--"))
                            .copied()
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
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
                Some("patch") if args.contains(&"held") => refuse(self.change || !workload),
                Some("patch") => refuse(self.relabel || !workload),
                Some("set" | "debug") => refuse(false),
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
    /// it is meant to find: the suspended-Job relabel, the seeded change to a person's control
    /// Job, and an admitted Deployment's pod.
    #[test]
    fn the_drill_holds_only_where_the_policies_hold() -> Result<()> {
        assert_eq!(drill_against(&Fake::default())?, Verdict::Held);
        for (fake, symptom) in [
            (
                Fake {
                    relabel: true,
                    ..Fake::default()
                },
                "admitted the control label on a suspended Job",
            ),
            (
                Fake {
                    change: true,
                    ..Fake::default()
                },
                "admitted a service account's change to a person's control Job",
            ),
            (
                Fake {
                    deployment: true,
                    ..Fake::default()
                },
                "admitted a labelled Deployment's pod",
            ),
        ] {
            assert_eq!(drill_against(&fake)?, Verdict::Broken(symptom.to_owned()));
        }
        Ok(())
    }

    /// A step that errs mid-run still leaves the webhook, the policies and both namespaces
    /// removed, and the error is what the run reports.
    #[test]
    fn a_failed_step_still_cleans_up() -> Result<()> {
        let fake = Fake {
            fails: Some("replicasets"),
            ..Fake::default()
        };
        let error = drill_against(&fake)
            .err()
            .context("the drill ran past a failed step")?;
        assert!(format!("{error:#}").contains("the cluster went away during replicasets"));
        let deleted = fake.deleted.borrow();
        let after = &deleted[deleted.len() - 4..];
        assert_eq!(
            after,
            [
                format!("delete validatingwebhookconfiguration {WEBHOOK}"),
                "delete -f -".to_owned(),
                format!("delete namespace {}", NAMESPACES[0]),
                format!("delete namespace {}", NAMESPACES[1]),
            ]
        );
        assert!(!*fake.webhook.borrow());
        Ok(())
    }
}
