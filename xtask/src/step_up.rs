//! Staging's step-up confirmation from CI (DEC-2087): `infra.yml`'s `step-up` action, which the
//! founder dispatches with the one string the web app shows beside a CLI-confirm code, runs
//! `mandate step-up confirm` or `confirm-connect` against staging's journal and store as one Job
//! made from the suspended `control-template` CronJob, the chart's own `control` run. This module
//! holds what reads no cluster: the string, the CLI's arguments, and the Job.

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

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
    let fields: Vec<&str> = input.split(':').collect();
    match fields.as_slice() {
        ["connect", challenge, code] => Ok(Confirm::Connect {
            challenge: ulid(challenge)?,
            code: code_of(code)?,
        }),
        ["approve", challenge, code, agent, approval] => Ok(Confirm::Approve {
            challenge: ulid(challenge)?,
            code: code_of(code)?,
            agent: id(agent, "agent")?,
            approval: id(approval, "approval")?,
        }),
        _ => bail!(
            "the step-up string is `connect:<challenge>:<code>` or \
             `approve:<challenge>:<code>:<agent>:<approval>`, as the web app shows it"
        ),
    }
}

/// `field` when it is a ULID: 26 of Crockford's base-32 digits, upper case, the first at most 7.
fn ulid(field: &str) -> Result<String> {
    let crockford = |b: u8| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b));
    ensure!(
        field.len() == 26
            && field.bytes().all(crockford)
            && field.as_bytes().first().is_some_and(|b| *b <= b'7'),
        "the challenge is not a ULID"
    );
    Ok(field.to_owned())
}

/// `field` when it is a code as the web app shows it, `XXXX-XXXX` or eight characters, letters
/// and digits alone; the CLI then checks it is Crockford's (DEC-2104 item 1).
fn code_of(field: &str) -> Result<String> {
    let bare: String = match field.split_once('-') {
        Some((first, second)) if first.len() == 4 => format!("{first}{second}"),
        Some(_) => bail!("the code is not `XXXX-XXXX`"),
        None => field.to_owned(),
    };
    ensure!(
        bare.len() == 8 && bare.bytes().all(|b| b.is_ascii_alphanumeric()),
        "the code is not `XXXX-XXXX`"
    );
    Ok(field.to_owned())
}

/// `field` when it is an id as the API names it: 1 to 128 letters, digits, `_` and `-`.
fn id(field: &str, what: &str) -> Result<String> {
    ensure!(
        (1..=128).contains(&field.len())
            && field
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "the {what} id is not 1 to 128 letters, digits, `_` or `-`"
    );
    Ok(field.to_owned())
}

/// How long the web app holds a challenge (DEC-2108 item 4): a confirmation it reads later is of
/// an expired challenge, and the founder asks for a new one.
pub(crate) const CHALLENGE_TTL_S: u64 = 300;

/// The challenge's age in whole seconds at `now_ms` (milliseconds since the epoch), read from its
/// ULID's 48-bit time, which the browser stamped when it asked for the challenge; `None` when the
/// ULID's time is after `now_ms`. A skewed browser clock skews it, so it only warns, never refuses.
pub(crate) fn challenge_age_s(challenge: &str, now_ms: u64) -> Option<u64> {
    const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut issued_ms: u64 = 0;
    for byte in challenge.bytes().take(10) {
        let digit = CROCKFORD.iter().position(|c| *c == byte)?;
        issued_ms = issued_ms
            .checked_mul(32)?
            .checked_add(u64::try_from(digit).ok()?)?;
    }
    now_ms.checked_sub(issued_ms).map(|age| age / 1000)
}

/// The arguments of `mandate` for `confirm`, as `workspace`'s `principal`, against the journal and
/// the artifact store the `control` run's environment names.
pub(crate) fn cli_args(confirm: &Confirm, workspace: &str, principal: &str) -> Vec<String> {
    let mut args = vec!["step-up"];
    let (challenge, code) = match confirm {
        Confirm::Connect { challenge, code } => {
            args.push("confirm-connect");
            args.extend(["--workspace", workspace, "--principal", principal]);
            (challenge, code)
        }
        Confirm::Approve {
            challenge,
            code,
            agent,
            approval,
        } => {
            args.push("confirm");
            args.extend(["--workspace", workspace, "--principal", principal]);
            args.extend(["--agent", agent, "--approval", approval]);
            (challenge, code)
        }
    };
    args.extend([
        "--challenge",
        challenge,
        "--code",
        code,
        "--journal",
        "$(MANDATE_JOURNAL_DSN)",
        "--store",
        "$(MANDATE_ARTIFACTS)",
    ]);
    args.into_iter().map(str::to_owned).collect()
}

/// The Job named `name` that `template`, the `control-template` CronJob as `kubectl get -o json`
/// reads it, makes with `args`: its job template's spec and labels, the CLI container's arguments
/// replaced, and nothing else changed. No owner reference, so it is the founder's Job.
///
/// # Errors
/// When the template is not a CronJob, or its pod does not hold exactly one `control` container.
pub(crate) fn job_from_template(template: &Value, name: &str, args: &[String]) -> Result<Value> {
    ensure!(
        template.get("kind").and_then(Value::as_str) == Some("CronJob")
            && template.pointer("/spec/suspend") == Some(&Value::Bool(true)),
        "{TEMPLATE} is not the suspended CronJob the chart renders"
    );
    let job = template
        .pointer("/spec/jobTemplate")
        .context("the template has no job template")?;
    let mut spec = job
        .get("spec")
        .cloned()
        .context("the job template has no spec")?;
    let containers = spec
        .pointer_mut("/template/spec/containers")
        .and_then(Value::as_array_mut)
        .context("the job template's pod has no containers")?;
    let mut controls = containers
        .iter_mut()
        .filter(|container| container.get("name").and_then(Value::as_str) == Some(CONTAINER));
    let (Some(control), None) = (controls.next(), controls.next()) else {
        bail!("the job template's pod does not hold exactly one `{CONTAINER}` container");
    };
    let names: Vec<&str> = control
        .get("env")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|variable| variable.get("name").and_then(Value::as_str))
        .collect();
    for needed in ["MANDATE_JOURNAL_DSN", "MANDATE_ARTIFACTS"] {
        ensure!(
            names.contains(&needed),
            "the `{CONTAINER}` container names no {needed}, which the CLI's arguments read"
        );
    }
    control["args"] = json!(args);
    let labels = job
        .pointer("/metadata/labels")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {"name": name, "labels": labels},
        "spec": spec
    }))
}

#[cfg(test)]
mod tests {
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

    /// The age comes from the ULID's time: `01KDEM…` is 2026-10-27, and a later instant reads the
    /// whole seconds since; a time after now has no age.
    #[test]
    fn a_challenges_age_is_read_from_its_ulid() {
        let at = |ms: u64| {
            const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
            let mut time = String::new();
            for shift in (0..10).rev() {
                let index = usize::try_from((ms >> (shift * 5)) & 31).unwrap_or_default();
                time.push(char::from(CROCKFORD[index]));
            }
            format!("{time}0000000000000000")
        };
        let issued = 1_793_000_000_000;
        assert_eq!(challenge_age_s(&at(issued), issued + 299_999), Some(299));
        assert_eq!(challenge_age_s(&at(issued), issued + 301_000), Some(301));
        assert_eq!(challenge_age_s(&at(issued), issued), Some(0));
        assert_eq!(challenge_age_s(&at(issued), issued - 1), None);
        assert_eq!(challenge_age_s("0IKDEM", issued), None);
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
                                    "env": [
                                        {"name": "MANDATE_JOURNAL_DSN", "value": "postgres://c@db/m"},
                                        {"name": "MANDATE_ARTIFACTS", "value": "/var/lib/mandate/artifacts"}
                                    ]
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
        let mut no_store = template();
        no_store["spec"]["jobTemplate"]["spec"]["template"]["spec"]["containers"][0]["env"] =
            json!([{"name": "MANDATE_JOURNAL_DSN", "value": "postgres://c@db/m"}]);
        for (case, template) in [
            ("not a CronJob", not_cron),
            ("two control containers", two),
            ("no control container", none),
            ("not suspended", active),
            ("no artifact store", no_store),
        ] {
            assert!(
                job_from_template(&template, "step-up-1", &["x".to_owned()]).is_err(),
                "{case} was accepted"
            );
        }
    }
}
