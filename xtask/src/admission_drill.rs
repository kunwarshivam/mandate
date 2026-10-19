//! The `admission` drill (E21-58, DEC-2016 item 2): the hosted clusters' kill-switch admission
//! policies, `deploy/gitops/base/kill-switch-admission.yaml`, applied as they are to the cell's
//! API server, with a signature webhook that is down and fails closed, as the hosted clusters'
//! does when policy-controller is unavailable.
//!
//! In namespaces of their own, labelled as the workspace's is, it checks that the founder's
//! kill-switch Job is admitted and its pod made, with the webhook down; that every other way to a
//! pod labelled `mandate.dev/process: control` is refused (a person's pod, a service account's
//! pod or Job, a Deployment's pod, a CronJob's Job, the label added later); that an unlabelled pod
//! is refused while the webhook is down; that a second workspace's namespace is covered by its
//! label alone; and that a finished kill-switch Job is cleaned up by foreground deletion. It leaves
//! the cell as it found it.

use std::fs;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, ensure};

use crate::cell::{NODE, Verdict};

/// The policies the drill applies: the file the hosted clusters' base applies.
const POLICIES: &str = "deploy/gitops/base/kill-switch-admission.yaml";

/// The clause the seeded drill removes: a CronJob may not own a control Job.
const CRONJOB_CLAUSE: &str = "\n        && !(has(object.metadata.ownerReferences)\n        && object.metadata.ownerReferences.exists(r, r.kind == 'CronJob'))";

/// The namespaces the drill makes, each labelled as a workspace's namespace is.
const NAMESPACES: [&str; 2] = ["admission-drill-1", "admission-drill-2"];

/// The label that puts a namespace under the signature webhooks and these policies.
const INCLUDE: &str = "policy.sigstore.dev/include=true";

/// A person, as the founder's CLI is, and a workload's service account, as the deployment
/// manager's is; both may do anything RBAC allows, so only admission can refuse them.
const FOUNDER: [&str; 2] = ["--as=founder", "--as-group=system:masters"];
const WORKLOAD: [&str; 2] = [
    "--as=system:serviceaccount:admission-drill-1:deploy-manager",
    "--as-group=system:masters",
];

/// A pinned image of this repository's; the drill never needs it pulled, only admitted.
const IMAGE: &str = "ghcr.io/kunwarshivam/mandate/mandate@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// The signature webhook, down: its service does not exist, it fails closed, and it skips exactly
/// the control label, as the hosted clusters' post-render patch makes it.
const DOWN_WEBHOOK: &str = r#"apiVersion: admissionregistration.k8s.io/v1
kind: ValidatingWebhookConfiguration
metadata: {name: admission-drill.policy.sigstore.dev}
webhooks:
  - name: admission-drill.policy.sigstore.dev
    admissionReviewVersions: [v1]
    sideEffects: None
    failurePolicy: Fail
    timeoutSeconds: 5
    namespaceSelector: {matchExpressions: [{key: policy.sigstore.dev/include, operator: In, values: ["true"]}]}
    objectSelector: {matchExpressions: [{key: mandate.dev/process, operator: NotIn, values: [control]}]}
    clientConfig: {service: {name: absent, namespace: admission-drill-1, path: /validations}}
    rules: [{apiGroups: [""], apiVersions: [v1], operations: [CREATE, UPDATE], resources: [pods]}]
"#;

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

fn pod(name: &str, process: &str) -> String {
    format!(
        "apiVersion: v1\nkind: Pod\nmetadata: {{name: {name}, labels: {{mandate.dev/process: {process}}}}}\n\
         spec: {{containers: [{{name: c, image: \"{IMAGE}\"}}]}}\n"
    )
}

fn job(name: &str) -> String {
    format!(
        "apiVersion: batch/v1\nkind: Job\nmetadata: {{name: {name}}}\nspec:\n  backoffLimit: 0\n  template:\n    \
         metadata: {{labels: {{mandate.dev/process: control}}}}\n    spec: {{restartPolicy: Never, \
         containers: [{{name: c, image: \"{IMAGE}\"}}]}}\n"
    )
}

/// Notes in `broken` that `what` got `got` rather than `want`.
fn expect(broken: &mut Vec<String>, what: &str, got: Outcome, want: Outcome) {
    if got != want {
        broken.push(format!("{what}: {got:?}, not {want:?}"));
    }
}

/// The drill's run against `kubectl`, with `policies` as the admission policies.
pub(crate) fn run(kubectl: Kubectl<'_>, policies: &str) -> Result<Verdict> {
    let create = |who: &[&str], namespace: &str, manifest: &str| -> Result<Outcome> {
        let mut args = who.to_vec();
        args.extend_from_slice(&["create", "--namespace", namespace, "-f", "-"]);
        Ok(outcome(kubectl(&args, Some(manifest))?))
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
    let made = |namespace: &str, selector: &str| -> Result<bool> {
        for _ in 0..30 {
            if pods(namespace, selector)? > 0 {
                return Ok(true);
            }
            sleep(Duration::from_secs(1));
        }
        Ok(false)
    };
    let (main, other) = (NAMESPACES[0], NAMESPACES[1]);
    for namespace in NAMESPACES {
        kubectl(&["create", "namespace", namespace], None)?;
        kubectl(&["label", "namespace", namespace, INCLUDE], None)?;
    }
    let (applied, text) = kubectl(&["apply", "-f", "-"], Some(policies))?;
    ensure!(applied, "applying the admission policies failed: {text}");
    let (applied, text) = kubectl(&["apply", "-f", "-"], Some(DOWN_WEBHOOK))?;
    ensure!(applied, "applying the down webhook failed: {text}");
    sleep(Duration::from_secs(3));

    let mut broken = Vec::new();
    expect(
        &mut broken,
        "the founder's kill-switch Job",
        create(&FOUNDER, main, &job("kill-switch"))?,
        Outcome::Admitted,
    );
    if !made(main, "job-name=kill-switch")? {
        broken.push("the kill switch's pod was not made with the webhook down".to_owned());
    }
    expect(
        &mut broken,
        "an unlabelled pod with the webhook down",
        create(&FOUNDER, main, &pod("unlabelled", "api"))?,
        Outcome::RefusedByWebhook,
    );
    expect(
        &mut broken,
        "a person's pod labelled control",
        create(&FOUNDER, main, &pod("person", "control"))?,
        Outcome::RefusedByPolicy,
    );
    expect(
        &mut broken,
        "a service account's pod labelled control",
        create(&WORKLOAD, main, &pod("workload", "control"))?,
        Outcome::RefusedByPolicy,
    );
    expect(
        &mut broken,
        "a service account's control Job",
        create(&WORKLOAD, main, &job("workload-job"))?,
        Outcome::RefusedByPolicy,
    );
    expect(
        &mut broken,
        "a labelled pod in a second workspace's namespace",
        create(&FOUNDER, other, &pod("second", "control"))?,
        Outcome::RefusedByPolicy,
    );
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
    sleep(Duration::from_secs(8));
    if pods(main, "app=fake-control")? > 0 {
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
    let mut from_cronjob = FOUNDER.to_vec();
    from_cronjob.extend_from_slice(&[
        "create",
        "job",
        "from-cronjob",
        "--from=cronjob/fake-control",
        "--namespace",
        main,
    ]);
    if outcome(kubectl(&from_cronjob, None)?) != Outcome::RefusedByPolicy {
        broken.push("admitted a Job a CronJob owns".to_owned());
    }
    kubectl(
        &[
            "delete",
            "validatingwebhookconfiguration",
            "admission-drill.policy.sigstore.dev",
        ],
        None,
    )?;
    expect(
        &mut broken,
        "an unlabelled pod with no webhook",
        create(&FOUNDER, main, &pod("relabelled", "api"))?,
        Outcome::Admitted,
    );
    let mut relabel = FOUNDER.to_vec();
    relabel.extend_from_slice(&[
        "label",
        "pod",
        "relabelled",
        "mandate.dev/process=control",
        "--overwrite",
        "--namespace",
        main,
    ]);
    expect(
        &mut broken,
        "the control label added to a pod",
        outcome(kubectl(&relabel, None)?),
        Outcome::RefusedByPolicy,
    );
    let (deleted, text) = kubectl(
        &[
            "delete",
            "job",
            "kill-switch",
            "--cascade=foreground",
            "--timeout=60s",
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
    kubectl(
        &[
            "delete",
            "validatingwebhookconfiguration",
            "admission-drill.policy.sigstore.dev",
            "--ignore-not-found",
        ],
        None,
    )?;
    for namespace in NAMESPACES {
        kubectl(&["delete", "namespace", namespace, "--wait=false"], None)?;
    }
    Ok(if broken.is_empty() {
        Verdict::Held
    } else {
        Verdict::Broken(broken.join("; "))
    })
}

/// The drill in the cell; seeded, the Job policy's CronJob clause is removed first.
pub(crate) fn drill(seeded: bool) -> Result<Verdict> {
    let policies = fs::read_to_string(POLICIES).with_context(|| format!("reading {POLICIES}"))?;
    let policies = if seeded {
        ensure!(
            policies.contains(CRONJOB_CLAUSE),
            "{POLICIES} no longer holds the CronJob clause the seeded drill removes"
        );
        policies.replace(CRONJOB_CLAUSE, "")
    } else {
        policies
    };
    run(&node_kubectl, &policies)
}
