//! Staging's step-up confirmation from CI (DEC-NNNN): `infra.yml`'s `step-up` action, which the
//! founder dispatches with the one string the web app shows beside a CLI-confirm code, runs
//! `mandate step-up confirm` or `confirm-connect` against staging's journal and store as one Job
//! made from the suspended `control-template` CronJob, the chart's own `control` run. This module
//! holds what reads no cluster: the string, the CLI's arguments, and the Job.

use anyhow::{Result, bail};
use serde_json::Value;

/// The suspended CronJob, in the workspace's namespace, whose job template is the `control` run.
pub(crate) const TEMPLATE: &str = "control-template";
/// The container in that template that runs the CLI.
const CONTAINER: &str = "control";
/// The environment variable `infra.yml` hands the dispatch's `confirm` input on in.
pub(crate) const CONFIRM_ENV: &str = "MANDATE_STEP_UP";

/// What the founder confirms: a connect's start, or an approval's grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Confirm {
    Connect {
        challenge: String,
        code: String,
    },
    Approve {
        challenge: String,
        code: String,
        agent: String,
        approval: String,
    },
}

/// The dispatch's string: `connect:<challenge>:<code>` or
/// `approve:<challenge>:<code>:<agent>:<approval>`.
///
/// # Errors
/// When it is neither form, before any cloud call.
pub(crate) fn parse(input: &str) -> Result<Confirm> {
    let _ = input;
    bail!("step-up is not built yet")
}

/// The arguments of `mandate` for `confirm`, as `workspace`'s `principal`, against the journal and
/// the artifact store the `control` run's environment names.
pub(crate) fn cli_args(confirm: &Confirm, workspace: &str, principal: &str) -> Vec<String> {
    let _ = (confirm, workspace, principal);
    Vec::new()
}

/// The Job named `name` that `template`, the `control-template` CronJob as `kubectl get -o json`
/// reads it, makes with `args`: its job template's spec and labels, the CLI container's arguments
/// replaced, and nothing else changed. No owner reference, so it is the founder's Job.
///
/// # Errors
/// When the template is not a CronJob, or its pod does not hold exactly one `control` container.
pub(crate) fn job_from_template(template: &Value, name: &str, args: &[String]) -> Result<Value> {
    let _ = (template, name, args);
    bail!("step-up is not built yet")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const CHALLENGE: &str = "01KDEMCHA11ENGE0000000000Z";
    const PRINCIPAL: &str = "01KDEM0000000000000000000F";
    const WORKSPACE: &str = "01KDEM0000000000000000000W";

    #[test]
    fn a_connect_and_an_approval_parse() -> Result<()> {
        assert_eq!(
            parse(&format!("connect:{CHALLENGE}:ABCD-EFGH"))?,
            Confirm::Connect {
                challenge: CHALLENGE.to_owned(),
                code: "ABCD-EFGH".to_owned(),
            }
        );
        assert_eq!(
            parse(&format!("approve:{CHALLENGE}:abcdefgh:agt_01X:apr-2_b"))?,
            Confirm::Approve {
                challenge: CHALLENGE.to_owned(),
                code: "abcdefgh".to_owned(),
                agent: "agt_01X".to_owned(),
                approval: "apr-2_b".to_owned(),
            }
        );
        Ok(())
    }

    /// Anything else is refused before any cloud call, so nothing the founder mistypes reaches a
    /// command line or the cluster.
    #[test]
    fn every_other_string_is_refused() {
        let long = "a".repeat(129);
        for input in [
            String::new(),
            format!("connect:{CHALLENGE}"),
            format!("connect:{CHALLENGE}:ABCD-EFGH:extra"),
            format!("approve:{CHALLENGE}:ABCD-EFGH:agt"),
            format!("revoke:{CHALLENGE}:ABCD-EFGH"),
            format!("Connect:{CHALLENGE}:ABCD-EFGH"),
            format!(" connect:{CHALLENGE}:ABCD-EFGH"),
            format!("connect:{CHALLENGE}:ABCD-EFGH\n"),
            "connect:01KDEMCHA11ENGE000000000:ABCD-EFGH".to_owned(),
            "connect:81KDEMCHA11ENGE0000000000Z:ABCD-EFGH".to_owned(),
            "connect:01KDEMCHA11ENGE0000000000U:ABCD-EFGH".to_owned(),
            format!("connect:{CHALLENGE}:ABC-DEFG"),
            format!("connect:{CHALLENGE}:ABCD EFGH"),
            format!("connect:{CHALLENGE}:ABCD-EFGH$"),
            format!("approve:{CHALLENGE}:ABCD-EFGH::apr"),
            format!("approve:{CHALLENGE}:ABCD-EFGH:agt:apr;rm"),
            format!("approve:{CHALLENGE}:ABCD-EFGH:agt:{long}"),
            format!("approve:{CHALLENGE}:ABCD-EFGH:$(id):apr"),
        ] {
            assert!(parse(&input).is_err(), "{input:?} was accepted");
        }
    }

    #[test]
    fn the_cli_runs_as_the_founder_against_the_control_runs_journal_and_store() {
        let connect = Confirm::Connect {
            challenge: CHALLENGE.to_owned(),
            code: "ABCD-EFGH".to_owned(),
        };
        assert_eq!(
            cli_args(&connect, WORKSPACE, PRINCIPAL),
            [
                "step-up",
                "confirm-connect",
                "--workspace",
                WORKSPACE,
                "--principal",
                PRINCIPAL,
                "--challenge",
                CHALLENGE,
                "--code",
                "ABCD-EFGH",
                "--journal",
                "$(MANDATE_JOURNAL_DSN)",
                "--store",
                "$(MANDATE_ARTIFACTS)",
            ]
        );
        let approve = Confirm::Approve {
            challenge: CHALLENGE.to_owned(),
            code: "ABCD-EFGH".to_owned(),
            agent: "agt_1".to_owned(),
            approval: "apr_1".to_owned(),
        };
        assert_eq!(
            cli_args(&approve, WORKSPACE, PRINCIPAL),
            [
                "step-up",
                "confirm",
                "--workspace",
                WORKSPACE,
                "--principal",
                PRINCIPAL,
                "--agent",
                "agt_1",
                "--approval",
                "apr_1",
                "--challenge",
                CHALLENGE,
                "--code",
                "ABCD-EFGH",
                "--journal",
                "$(MANDATE_JOURNAL_DSN)",
                "--store",
                "$(MANDATE_ARTIFACTS)",
            ]
        );
    }

    fn template() -> Value {
        json!({
            "apiVersion": "batch/v1",
            "kind": "CronJob",
            "metadata": {
                "name": TEMPLATE,
                "namespace": "ws-ws1",
                "uid": "u-1",
                "resourceVersion": "7",
                "labels": {"mandate.dev/process": "control"}
            },
            "spec": {
                "suspend": true,
                "schedule": "0 0 31 2 *",
                "jobTemplate": {
                    "metadata": {"labels": {"mandate.dev/process": "control"}},
                    "spec": {
                        "backoffLimit": 0,
                        "activeDeadlineSeconds": 120,
                        "template": {
                            "metadata": {"labels": {"mandate.dev/process": "control"}},
                            "spec": {
                                "restartPolicy": "Never",
                                "initContainers": [{"name": "credentials", "args": ["keep"]}],
                                "containers": [{
                                    "name": "control",
                                    "image": "ghcr.io/x/mandate@sha256:aa",
                                    "args": ["--help"],
                                    "env": [{"name": "MANDATE_JOURNAL_DSN", "value": "postgres://c@db/m"}]
                                }]
                            }
                        }
                    }
                }
            }
        })
    }

    /// The Job is the template's job, with the CLI's arguments alone changed: the same image,
    /// role, environment, mounts and limits, and no owner, so the kill-switch admission policy
    /// (which refuses a control Job a CronJob owns) admits it as the founder's.
    #[test]
    fn the_job_is_the_templates_with_the_arguments_alone_changed() -> Result<()> {
        let args = vec!["step-up".to_owned(), "confirm-connect".to_owned()];
        let job = job_from_template(&template(), "step-up-42", &args)?;
        let mut expected = template()["spec"]["jobTemplate"]["spec"].clone();
        expected["template"]["spec"]["containers"][0]["args"] = json!(args);
        assert_eq!(
            job,
            json!({
                "apiVersion": "batch/v1",
                "kind": "Job",
                "metadata": {
                    "name": "step-up-42",
                    "labels": {"mandate.dev/process": "control"}
                },
                "spec": expected
            })
        );
        Ok(())
    }

    #[test]
    fn a_template_that_is_not_the_control_run_is_refused() {
        let mut not_cron = template();
        not_cron["kind"] = json!("Deployment");
        let mut two = template();
        two["spec"]["jobTemplate"]["spec"]["template"]["spec"]["containers"] =
            json!([{"name": "control"}, {"name": "control"}]);
        let mut none = template();
        none["spec"]["jobTemplate"]["spec"]["template"]["spec"]["containers"] =
            json!([{"name": "runtime"}]);
        let mut active = template();
        active["spec"]["suspend"] = json!(false);
        for (case, template) in [
            ("not a CronJob", not_cron),
            ("two control containers", two),
            ("no control container", none),
            ("not suspended", active),
        ] {
            assert!(
                job_from_template(&template, "step-up-1", &["x".to_owned()]).is_err(),
                "{case} was accepted"
            );
        }
    }
}
