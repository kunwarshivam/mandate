//! The `executor` drill (E21-30, DEC-2035): an agent deployed on a recorded paper connection, and
//! the deployment manager starting that account's executor, which the drill waits on until its pod
//! runs and its process answers `livez`. Before it, the scheduler's ticks (E21-29): the run's
//! workspace is the one the chart names the scheduler, and its clock stream, exported twice a few
//! seconds apart, must have grown, so the ticks the runtime and the executor read arrive. Paper and
//! fixtures only: the connect check and the executor reach the in-process fixture broker
//! (DEC-2007), and no credential is read.
//!
//! Three one-off Jobs, each a fresh `/work`: the executor's connect check (`checked <event id>`);
//! the workspace opened and founded, and the connection's record shown (`code <code>`); then the
//! record confirmed with that code and the demo's mandate registered, created, confirmed and
//! deployed in one pod, so the artifact store `version create` writes is the one `version confirm`
//! and `agent deploy` read. The deployment is `AgentDeployed` version 2 exactly when the record
//! committed (journal spec §9.24 rule 201), and only then does the manager start an executor.

use std::fs;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::scenario::{
    AGENT, FOUNDER, Gesture, INSTRUMENT_FILE, MANDATE_FILE, confirm_code, deploy_code,
    gestures_job, lines_of, point_api, reference,
};
use super::{NAMESPACE, Verdict, kubectl, render, run_id, run_job};

/// The connection the demo's mandate names; [`tests`] checks the file still names it.
pub const CONNECTION: &str = "conn_demo_paper_01";
const JOURNAL: &str = "$(MANDATE_JOURNAL_DSN)";
const STORE: &str = "/work/store";
const INPUTS: &str = "/in";
/// The polls, two seconds apart, for the manager's executor pod to run and answer `livez`; a
/// seeded run waits all of them before it names the pod missing.
const POLLS: u32 = 45;
const MANAGED_BY: &str = "app.kubernetes.io/managed-by=mandate-deploy-manager";
const SUBJECT: &str = "mandate.dev/subject";
/// The seconds between the clock stream's two exports: the scheduler ticks once a second, so the
/// second export holds more records than the first however the Jobs are scheduled.
const TICK_GAP: Duration = Duration::from_secs(3);
/// The export's file, in the Job's own `/work`.
const CLOCK_SEGMENT: &str = "/work/clock.seg";
/// The executor's own probe, as its pod's `livez` probe runs it (`processes.json`, DEC-994 item 5).
const LIVEZ: [&str; 5] = [
    "/mandate-executord",
    "probe",
    "livez",
    "--probe",
    "/tmp/process.sock",
];

/// The run's workspace: a ULID, fresh for each run id, since the manager and API host only a ULID
/// and a confirmed version refuses a second confirmation.
pub fn workspace_of(id: &str) -> String {
    format!("01KDEX{id:0>10.10}000000000X")
}

/// The run's account stream: a ULID, fresh for each run id, so no other connection binds it.
pub fn account_of(id: &str) -> String {
    format!("01KACT{id:0>10.10}000000000A")
}

/// The connect check's arguments, for the executor image's `mandate-executord`.
pub fn check_args(workspace: &str, account: &str) -> Vec<String> {
    [
        "connect-check",
        "--workspace",
        workspace,
        "--account",
        account,
        "--connection",
        CONNECTION,
        "--pii-ref",
        "pii_01KDEX0000000000000000000P",
    ]
    .map(String::from)
    .to_vec()
}

/// The scheduler's stream for `workspace` (journal spec §2), the one the runtime and executor
/// read their ticks from.
pub fn clock_of(workspace: &str) -> String {
    format!("clock:{workspace}")
}

/// The arguments of one export of the workspace's clock stream, for the `mandate` CLI.
pub fn clock_export_args(workspace: &str) -> Vec<String> {
    strings(&[
        "journal",
        "export",
        &clock_of(workspace),
        CLOCK_SEGMENT,
        "--journal",
        JOURNAL,
    ])
}

/// The last sequence number an export's logs report for `stream`: `None` when no line reports
/// that stream's export.
pub fn exported_through(logs: &str, stream: &str) -> Option<u64> {
    let head = format!("exported: stream {stream}, seq 1 to ");
    logs.lines().find_map(|l| {
        let (_, rest) = l.split_once(&head)?;
        rest.split(',').next()?.trim().parse().ok()
    })
}

/// Whether two exports show the scheduler ticking: the first holds its `StreamOpened` and at
/// least one record after it, and the second strictly more.
pub fn ticking(first: Option<u64>, second: Option<u64>) -> bool {
    matches!((first, second), (Some(a), Some(b)) if a >= 2 && b > a)
}

/// The event id a connect check's logs name as passing.
pub fn checked_in(logs: &str) -> Option<&str> {
    logs.lines()
        .filter_map(|l| l.rsplit("] ").next())
        .find_map(|l| l.trim().strip_prefix("checked "))
}

fn owner(workspace: &str, args: &[&str]) -> Vec<String> {
    let user = FOUNDER.to_ascii_lowercase();
    args.iter()
        .copied()
        .chain([
            "--workspace",
            workspace,
            "--user",
            &user,
            "--journal",
            JOURNAL,
            "--store",
            STORE,
        ])
        .map(String::from)
        .collect()
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

fn gesture(index: usize, shows: &'static str, args: Vec<String>, expect: String) -> Gesture {
    Gesture {
        container: format!("g{index:02}-{}", args[..2].join("-")),
        shows,
        command: None,
        args,
        expect,
    }
}

/// The record's arguments, with `code` once it is known.
fn record(workspace: &str, account: &str, checked: &str, code: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "connection",
        "record",
        "--environment",
        "paper",
        "--connection",
        CONNECTION,
        "--broker",
        "alpaca",
        "--scope",
        "trading",
        "--account-ref",
        account,
        "--checked",
        checked,
    ];
    if let Some(code) = code {
        args.extend(["--code", code]);
    }
    owner(workspace, &args)
}

/// The second Job's gestures: the workspace opened and founded, and the record shown.
pub fn opening(workspace: &str, account: &str, checked: &str) -> Vec<Gesture> {
    let mut all = workspace_opened(workspace);
    all.push(gesture(
        3,
        "the connection's record and its code, shown (nothing committed)",
        record(workspace, account, checked, None),
        "code ".into(),
    ));
    all
}

/// The workspace's paper control stream, opened, and the founder's membership: what both runs
/// need before the deployment's gestures, the seeded run included, so its only difference is the
/// connection left unrecorded.
pub fn workspace_opened(workspace: &str) -> Vec<Gesture> {
    let target = ["--journal", JOURNAL, "--store", STORE];
    let open = [
        "workspace",
        "open",
        "--workspace",
        workspace,
        "--environment",
        "paper",
    ];
    let found = [
        "workspace",
        "found",
        "--workspace",
        workspace,
        "--member",
        FOUNDER,
    ];
    vec![
        gesture(
            1,
            "the workspace's paper control stream, opened",
            strings(&[&open[..], &target].concat()),
            format!("opened ctl:{workspace}"),
        ),
        gesture(
            2,
            "the founder's membership",
            strings(&[&found[..], &["--method", "oidc"], &target].concat()),
            format!("founded ctl:{workspace} for member {FOUNDER}"),
        ),
    ]
}

/// The third Job's gestures: the record committed when `recorded` names its check and code, then
/// the demo's mandate registered, created, confirmed and deployed.
pub fn deployment(
    workspace: &str,
    version: &str,
    recorded: Option<(&str, &str, &str)>,
) -> Vec<Gesture> {
    let mut all = Vec::new();
    if let Some((account, checked, code)) = recorded {
        all.push((
            "the connection, recorded with the code shown",
            record(workspace, account, checked, Some(code)),
            format!("recorded {CONNECTION} as event"),
        ));
    }
    let instrument = format!("{INPUTS}/instrument.json");
    let mandate = format!("{INPUTS}/mandate.json");
    let (confirm, deploy) = (confirm_code(version), deploy_code(AGENT, version));
    all.extend([
        (
            "SPY's instrument snapshot, registered",
            owner(
                workspace,
                &[
                    "config",
                    "register",
                    "--kind",
                    "instrument_snapshot",
                    &instrument,
                ],
            ),
            "registered as event".to_string(),
        ),
        (
            "the moving-average crossover model, registered",
            owner(
                workspace,
                &["model", "register", "quant.ma_crossover", "1.0.0"],
            ),
            "registered as event".into(),
        ),
        (
            "the envelope's version, created",
            owner(workspace, &["version", "create", &mandate]),
            format!("created {version}"),
        ),
        (
            "the envelope, confirmed",
            owner(
                workspace,
                &["version", "confirm", version, "--code", &confirm],
            ),
            "confirmed as event".into(),
        ),
        (
            "the agent, deployed",
            owner(
                workspace,
                &["agent", "deploy", AGENT, version, "--code", &deploy],
            ),
            "deployed as event".into(),
        ),
    ]);
    all.into_iter()
        .enumerate()
        .map(|(n, (shows, args, expect))| gesture(n + 1, shows, args, expect))
        .collect()
}

/// The manager's executor pod for `account` in `pods` (a `kubectl get pods -o json` list): `None`
/// when there is none, else its name once it is `Running` with every container started, and `None`
/// inside while it is not.
pub fn executor_running(pods: &Value, account: &str) -> Option<Option<String>> {
    let pod = pods["items"]
        .as_array()?
        .iter()
        .find(|p| p["metadata"]["annotations"][SUBJECT] == account)?;
    let statuses = pod["status"]["containerStatuses"].as_array();
    let started =
        statuses.is_some_and(|all| !all.is_empty() && all.iter().all(|c| c["started"] == true));
    let running = pod["status"]["phase"] == "Running" && started;
    Some(
        running
            .then(|| pod["metadata"]["name"].as_str().map(str::to_owned))
            .flatten(),
    )
}

/// The drill. In seeded mode the connect check is skipped, so the connection is never recorded,
/// the deployment is version 1, and the manager starts no executor.
pub fn drill(seeded: bool) -> Result<Verdict> {
    let id = run_id()?;
    let (workspace, account) = (workspace_of(&id), account_of(&id));
    point_api(&workspace)?;
    let mandate = fs::read(MANDATE_FILE).with_context(|| format!("reading {MANDATE_FILE}"))?;
    let version = reference(&mandate);
    let files = [
        (
            "mandate.json",
            String::from_utf8(mandate).context("the mandate is not UTF-8")?,
        ),
        (
            "instrument.json",
            fs::read_to_string(INSTRUMENT_FILE)
                .with_context(|| format!("reading {INSTRUMENT_FILE}"))?,
        ),
    ];
    let recorded = if seeded {
        let (done, logs) = gestures(&id, "o", &workspace_opened(&workspace), &files)?;
        if !done {
            return Ok(Verdict::Broken(format!(
                "the workspace failed to open:\n{logs}"
            )));
        }
        None
    } else {
        let check = render(
            &[
                format!("run.id=\"{id}c\""),
                "run.process=\"connect-check\"".into(),
                format!("run.args={}", json!(check_args(&workspace, &account))),
            ],
            Some("templates/run.yaml"),
        )?;
        let (done, logs) = run_job(check)?;
        let Some(checked) = checked_in(&logs).filter(|_| done).map(str::to_owned) else {
            return Ok(Verdict::Broken(format!(
                "the connect check did not pass:\n{logs}"
            )));
        };
        let (done, logs) = gestures(&id, "o", &opening(&workspace, &account, &checked), &files)?;
        let code = lines_of(&logs, "g03-connection-record")
            .iter()
            .find_map(|l| l.strip_prefix("code "))
            .map(str::to_owned);
        let Some(code) = code.filter(|_| done) else {
            return Ok(Verdict::Broken(format!(
                "the workspace or the record's code failed:\n{logs}"
            )));
        };
        Some((checked, code))
    };
    if let Some(broken) = ticks(&id, &workspace)? {
        return Ok(Verdict::Broken(broken));
    }
    let recorded = recorded
        .as_ref()
        .map(|(checked, code)| (account.as_str(), checked.as_str(), code.as_str()));
    let (done, logs) = gestures(
        &id,
        "d",
        &deployment(&workspace, &version, recorded),
        &files,
    )?;
    if !done {
        return Ok(Verdict::Broken(format!(
            "the deployment's gestures failed:\n{logs}"
        )));
    }
    let selector = format!("{MANAGED_BY},mandate.dev/process=executor");
    let mut found = None;
    for _ in 0..POLLS {
        let pods = kubectl(&[
            "get",
            "pods",
            "--namespace",
            NAMESPACE,
            "--selector",
            &selector,
            "-o",
            "json",
        ])?;
        found = executor_running(
            &serde_json::from_str(&pods).context("the executor pods")?,
            &account,
        );
        if let Some(Some(pod)) = &found {
            let mut exec = vec!["exec", "--namespace", NAMESPACE, pod.as_str(), "--"];
            exec.extend(LIVEZ);
            if kubectl(&exec).is_ok() {
                eprintln!(
                    "    the manager's executor for account {account} runs and is live; its \
                     readiness is lane X's (E21-30)"
                );
                return Ok(Verdict::Held);
            }
        }
        sleep(Duration::from_secs(2));
    }
    Ok(Verdict::Broken(match found {
        None => {
            format!("no executor pod for account {account}: the deployment manager started none")
        }
        Some(_) => format!("the executor pod for account {account} never ran and answered livez"),
    }))
}

/// The scheduler's ticks on the run's workspace (E21-29): two exports of its clock stream,
/// [`TICK_GAP`] apart, each a one-off `control` Job; `None` when the second holds more records than
/// the first, else why not.
fn ticks(id: &str, workspace: &str) -> Result<Option<String>> {
    let stream = clock_of(workspace);
    let mut seen = Vec::new();
    for (n, suffix) in ["k1", "k2"].into_iter().enumerate() {
        if n > 0 {
            sleep(TICK_GAP);
        }
        let export = render(
            &[
                format!("run.id=\"{id}{suffix}\""),
                format!("run.args={}", json!(clock_export_args(workspace))),
            ],
            Some("templates/run.yaml"),
        )?;
        let (done, logs) = run_job(export)?;
        let through = exported_through(&logs, &stream).filter(|_| done);
        if through.is_none() {
            return Ok(Some(format!(
                "the scheduler's stream {stream} could not be exported:\n{logs}"
            )));
        }
        seen.push(through);
    }
    let (first, second) = (
        seen.first().copied().flatten(),
        seen.get(1).copied().flatten(),
    );
    if !ticking(first, second) {
        return Ok(Some(format!(
            "the scheduler is not ticking {stream}: its exports end at seq {first:?}, then {second:?}"
        )));
    }
    eprintln!("    the scheduler ticks {stream}: seq {first:?}, then {second:?}");
    Ok(None)
}

/// Runs `all` as one gestures Job of the `control` process, run `{id}{suffix}`.
fn gestures(
    id: &str,
    suffix: &str,
    all: &[Gesture],
    files: &[(&str, String)],
) -> Result<(bool, String)> {
    let first = all.first().context("a Job with no gesture")?;
    let rendered = render(
        &[
            format!("run.id=\"{id}{suffix}\""),
            format!("run.args={}", json!(first.args)),
        ],
        Some("templates/run.yaml"),
    )?;
    let all: Vec<&Gesture> = all.iter().collect();
    run_job(gestures_job(
        rendered,
        &all,
        &format!("executor-in-{id}{suffix}"),
        files,
        NAMESPACE,
    )?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// The drill records the connection the demo's mandate names, so the deployment binds it
    /// (journal spec §9.24 rule 200).
    #[test]
    fn the_drill_records_the_connection_the_mandate_names() {
        let mandate: Value =
            serde_json::from_slice(&fs::read(format!("../{MANDATE_FILE}")).unwrap()).unwrap();
        assert_eq!(mandate["connection_id"], CONNECTION);
        assert_eq!(mandate["environment"], "paper");
    }

    /// The run's workspace and account are ULIDs (journal spec §2), distinct per run id.
    #[test]
    fn the_runs_ids_are_ulids() {
        let crockford = |s: &str| {
            s.len() == 26
                && s.starts_with('0')
                && s.chars()
                    .all(|c| c.is_ascii_digit() || (c.is_ascii_uppercase() && !"ILOU".contains(c)))
        };
        for id in ["0123456789", "42"] {
            assert!(crockford(&workspace_of(id)), "{}", workspace_of(id));
            assert!(crockford(&account_of(id)), "{}", account_of(id));
        }
        assert_ne!(workspace_of("1"), workspace_of("2"));
    }

    /// The export's report is read for its own stream alone, and a later line of another stream
    /// or a failed export reports nothing.
    #[test]
    fn the_clock_exports_last_seq_is_read_from_the_logs() {
        let stream = clock_of("01KDEX0000000042000000000X");
        let line = format!(
            "[pod/x/run] exported: stream {stream}, seq 1 to 17, last hash ab, to /work/clock.seg"
        );
        assert_eq!(exported_through(&line, &stream), Some(17));
        let other = "exported: stream clock:01KDEX0000000043000000000X, seq 1 to 9, last hash ab";
        assert_eq!(exported_through(other, &stream), None);
        assert_eq!(exported_through("stream_not_found", &stream), None);
        let args = clock_export_args("w");
        assert_eq!(args[..3], ["journal", "export", "clock:w"]);
        assert!(args.windows(2).any(|w| w == ["--journal", JOURNAL]));
    }

    /// Ticking is a second export strictly past a first that holds a record after the stream's
    /// opening: a stalled, reset, opened-only, or missing stream is not.
    #[test]
    fn ticking_needs_a_grown_stream() {
        assert!(ticking(Some(2), Some(3)));
        assert!(ticking(Some(40), Some(43)));
        for (a, b) in [
            (Some(5), Some(5)),
            (Some(5), Some(4)),
            (Some(1), Some(4)),
            (None, Some(4)),
            (Some(4), None),
            (None, None),
        ] {
            assert!(!ticking(a, b), "{a:?} then {b:?}");
        }
    }

    #[test]
    fn the_checked_event_is_read_from_the_logs() {
        let id = "01KCHK0000000000000000000A";
        assert_eq!(
            checked_in(&format!("[pod/x/connect-check] checked {id}\n")),
            Some(id)
        );
        assert_eq!(checked_in(&format!("check_failed {id}")), None);
    }

    /// The record is committed before the deployment, so rule 201 holds when it is written; the
    /// seeded run has no record and so deploys at version 1.
    #[test]
    fn the_record_comes_before_the_deployment() {
        let with = deployment("w", "sha256:v", Some(("a", "c", "k")));
        assert_eq!(with[0].args[..2], ["connection", "record"]);
        assert!(with[0].args.windows(2).any(|w| w == ["--code", "k"]));
        assert_eq!(with.last().unwrap().args[..2], ["agent", "deploy"]);
        let without = deployment("w", "sha256:v", None);
        assert!(without.iter().all(|g| g.args[0] != "connection"));
        let names: Vec<_> = with.iter().map(|g| g.container.clone()).collect();
        let unique: BTreeSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "{names:?}");
    }

    /// Both runs open the workspace's control stream and found its member before the deployment,
    /// so the seeded run reaches the deployment manager and differs only by the missing record.
    #[test]
    fn both_runs_open_the_workspace_first() {
        let opened = workspace_opened("w");
        let heads: Vec<_> = opened.iter().map(|g| g.args[..2].to_vec()).collect();
        assert_eq!(heads, [["workspace", "open"], ["workspace", "found"]]);
        let full = opening("w", "a", "c");
        assert_eq!(full.len(), opened.len() + 1);
        for (a, b) in full.iter().zip(&opened) {
            assert_eq!(a.args, b.args);
        }
        assert_eq!(full.last().unwrap().args[..2], ["connection", "record"]);
    }

    #[test]
    fn the_executor_is_found_by_its_account_once_it_runs() {
        let pod = |subject: &str, phase: &str, started: bool| {
            json!({"metadata": {"name": format!("executor-{subject}"),
                                "annotations": {SUBJECT: subject}},
                   "status": {"phase": phase,
                              "containerStatuses": [{"name": "executor", "started": started}]}})
        };
        let pods = json!({"items": [pod("other", "Running", true), pod("acct", "Pending", false)]});
        assert_eq!(executor_running(&pods, "acct"), Some(None));
        assert_eq!(executor_running(&pods, "none"), None);
        let pods = json!({"items": [pod("acct", "Running", false)]});
        assert_eq!(executor_running(&pods, "acct"), Some(None), "not started");
        let pods = json!({"items": [pod("acct", "Failed", true)]});
        assert_eq!(executor_running(&pods, "acct"), Some(None), "stopped");
        let pods = json!({"items": [pod("acct", "Running", true)]});
        assert_eq!(
            executor_running(&pods, "acct"),
            Some(Some("executor-acct".to_owned()))
        );
        let bare = json!({"items": [{"metadata": {"name": "executor-acct",
                                                  "annotations": {SUBJECT: "acct"}},
                                     "status": {"phase": "Running", "containerStatuses": []}}]});
        assert_eq!(executor_running(&bare, "acct"), Some(None), "no container");
    }
}
