//! `cargo xtask cell demo --scenario demo` (E21-54, DEC-2002): the demo's story, its six steps in
//! order, on the local cell, with paper and demo data only and no credential. Every gesture whose
//! pieces are on `main` runs for real, in one Job of the cell's own `control` process, so the
//! artifact store its gestures share lives as long as they do; every link that is not on `main` is
//! printed as a gap naming the PR or story that closes it, never faked. The scenario ends, in the
//! same pod, by exporting the workspace's control stream and verifying it by journal spec §11
//! against the artifact store its gestures wrote, so every artifact a record names is checked.
//!
//! Each run takes a fresh workspace, `demo<run id>`, because a confirmed version is refused a
//! second confirmation (`version_invalid`), so a run never depends on what an earlier one left.
use std::collections::BTreeMap;
use std::env;
use std::fs;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    API_LOCAL_PORT, API_PROCESS, CELL_REQUIRED_ENV, DEMO_WORKSPACE, NAMESPACE, NODE, api_image_tag,
    api_port_forward, http_status, kubectl, processes, render, run_id, run_job, stub_image_tag,
    tools_missing, up,
};
use crate::output;

/// The scenarios `--scenario` takes.
pub const SCENARIOS: &[&str] = &["demo"];
/// The demo's mandate, its instrument snapshot, and where each is mounted in the gestures' pod.
/// Both are stored as canonical JSON (journal spec §4), so a version is the SHA-256 of the file.
pub const MANDATE_FILE: &str = "deploy/cell/demo/mandate.json";
pub const INSTRUMENT_FILE: &str = "deploy/cell/demo/instrument.json";
const INPUTS: &str = "/in";
/// The journal and the artifact store every gesture names: the `control` process's DSN, expanded
/// by Kubernetes from its environment, and the pod's own `/work`, which all its containers share.
const JOURNAL: &str = "$(MANDATE_JOURNAL_DSN)";
const STORE: &str = "/work/store";
const SEGMENT: &str = "/work/segment.jsonl";
/// The founding member: an opaque ULID (identity spec §3.1), never a name. The CLI's owner
/// gestures take the lower-case form, which is all `--user` accepts (DEC-527 item 3).
pub const FOUNDER: &str = "01K7DEM00000000000000F0VND";
/// The demo's agent.
pub const AGENT: &str = "demo-spy";
/// How long the gestures' Job may run: the egress gate, the credentials copy and every gesture,
/// one after another. Under the 300 s `run_job` polls for, so the Job ends, and the report is
/// written, before the polling gives up.
const DEADLINE_S: u64 = 240;

/// One of the demo's six steps and what it is made of.
#[derive(Debug)]
pub struct Step {
    pub number: u8,
    pub title: &'static str,
    pub parts: Vec<Part>,
}

/// A piece of a step: a CLI gesture the scenario runs in the cell, a check of the cell's API, or
/// a link not on `main` yet.
#[derive(Debug)]
pub enum Part {
    Gesture(Gesture),
    Api(ApiCheck),
    Gap(Gap),
}

/// A link of the story not on `main` yet (DEC-2004): what it would show, what of it is already on
/// `main`, and every lane whose work it still waits on. A gap with no lane waits on nobody, which
/// the plan's tests refuse.
#[derive(Debug)]
pub struct Gap {
    pub what: &'static str,
    pub on_main: &'static str,
    pub waits: Vec<Wait>,
}

/// One lane's work a gap waits on: the lane, the backlog story, the open PR when there is one, and
/// what the lane still has to land for this gap.
#[derive(Debug)]
pub struct Wait {
    pub lane: &'static str,
    pub story: &'static str,
    pub pr: Option<u32>,
    pub lands: &'static str,
}

impl Wait {
    /// The lane, its story and its PR, as the report names them: `SI (E21-52, #1591)`.
    pub fn named(&self) -> String {
        match self.pr {
            Some(pr) => format!("{} ({}, #{pr})", self.lane, self.story),
            None => format!("{} ({})", self.lane, self.story),
        }
    }
}

/// How much of a step runs: all of it, some of it with gaps, or none of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Partial,
    Skipped,
}

impl Readiness {
    pub fn word(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Partial => "partial",
            Self::Skipped => "skipped",
        }
    }
}

/// A step is ready with no gap, skipped when it is nothing but gaps, and partial otherwise.
pub fn readiness(step: &Step) -> Readiness {
    let gaps = step
        .parts
        .iter()
        .filter(|p| matches!(p, Part::Gap(_)))
        .count();
    match gaps {
        0 => Readiness::Ready,
        n if n == step.parts.len() => Readiness::Skipped,
        _ => Readiness::Partial,
    }
}

/// The lanes `step`'s gaps wait on, each once, in the order the step first names them.
pub fn lanes(step: &Step) -> Vec<&'static str> {
    let mut lanes = Vec::new();
    for part in &step.parts {
        if let Part::Gap(gap) = part {
            for wait in &gap.waits {
                if !lanes.contains(&wait.lane) {
                    lanes.push(wait.lane);
                }
            }
        }
    }
    lanes
}

/// Every wait of every gap, grouped by lane: the step it holds back, the gap, and the wait.
pub fn by_lane(steps: &[Step]) -> BTreeMap<&'static str, Vec<(u8, &Gap, &Wait)>> {
    let mut lanes: BTreeMap<&'static str, Vec<(u8, &Gap, &Wait)>> = BTreeMap::new();
    for step in steps {
        for part in &step.parts {
            if let Part::Gap(gap) = part {
                for wait in &gap.waits {
                    lanes
                        .entry(wait.lane)
                        .or_default()
                        .push((step.number, gap, wait));
                }
            }
        }
    }
    lanes
}

/// The waits several gaps share: a session from the stub issuer, the agent's runtime started by
/// the deployment manager, web/ driven against the cell, and the executor in the cell.
const SI_SESSION: Wait = Wait {
    lane: "SI",
    story: "E21-52",
    pr: Some(1591),
    lands: "the stub issuer's entry point, so the API opens a session (DEC-2100 item 9)",
};
const D1_RUNTIME: Wait = Wait {
    lane: "D1",
    story: "E21-33",
    pr: None,
    lands: "the deployment manager reads a deployment's account from AgentDeployed version 2 and \
            starts its runtime (DEC-1171, claim #1491); until then it starts none",
};
const W2_WEB: Wait = Wait {
    lane: "W2",
    story: "E11-9",
    pr: None,
    lands: "web/ sends the demo's gestures through the API, and `cell demo` drives web/ against \
            the cell (claim #1443)",
};
const X_EXECUTOR: Wait = Wait {
    lane: "X",
    story: "E21-30",
    pr: None,
    lands: "the executor as its own process in the cell, reading the runtime through the journal \
            and applying the kill switch first (DEC-2003, claim #1498)",
};

fn wait(lane: &'static str, story: &'static str, pr: Option<u32>, lands: &'static str) -> Wait {
    Wait {
        lane,
        story,
        pr,
        lands,
    }
}

/// One request to the cell's workspace API through `cell demo`'s port-forward, and the status it
/// must answer: a check of what the API serves today, never a stand-in for a step it cannot do.
#[derive(Debug)]
pub struct ApiCheck {
    pub shows: &'static str,
    pub path: String,
    pub status: u16,
}

/// One CLI command in the cell, the text its success prints, and what it shows the demo. A gesture
/// ran only when its container exited 0 and printed `expect`; a shown code is checked this way
/// after the Job, and the CLI itself refuses a typed code that differs from the one it shows.
#[derive(Debug)]
pub struct Gesture {
    pub container: String,
    pub shows: &'static str,
    pub args: Vec<String>,
    pub expect: String,
}

/// `sha256:` and the hex SHA-256 of `bytes`: an artifact reference (journal spec §4).
pub fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(&Sha256::digest(bytes)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// DEC-530 item 4's code for a gesture whose canonical JSON is `canonical`: the first eight hex
/// digits of its SHA-256. The scenario types the code the CLI has just shown, as the owner would.
pub fn code(canonical: &str) -> String {
    hex(&Sha256::digest(canonical.as_bytes()))[..8].to_owned()
}

fn confirm_code(version: &str) -> String {
    code(&format!(
        r#"{{"gesture":"mandate_confirm","mandate_version":"{version}"}}"#
    ))
}

fn deploy_code(agent: &str, version: &str) -> String {
    code(&format!(
        r#"{{"agent_id":"{agent}","gesture":"agent_deploy","mandate_version":"{version}"}}"#
    ))
}

/// The demo's six steps for `workspace`, given the bytes of the demo's mandate.
pub fn plan(workspace: &str, mandate: &[u8]) -> Vec<Step> {
    let version = reference(mandate);
    let user = FOUNDER.to_ascii_lowercase();
    let with = |args: &[&str], extra: &[&str]| -> Vec<String> {
        args.iter().chain(extra).map(|a| a.to_string()).collect()
    };
    let target = ["--journal", JOURNAL, "--store", STORE];
    let owner = |args: &[&str]| {
        with(
            args,
            &[&["--workspace", workspace, "--user", &user][..], &target].concat(),
        )
    };
    let stream = format!("ctl:{workspace}");
    let mut index = 0;
    let mut gesture = |shows: &'static str, args: Vec<String>, expect: String| {
        index += 1;
        Part::Gesture(Gesture {
            container: format!("g{index:02}-{}", args[..2].join("-")),
            shows,
            args,
            expect,
        })
    };
    let mandate_in = format!("{INPUTS}/mandate.json");
    let instrument_in = format!("{INPUTS}/instrument.json");
    let (confirm, deploy) = (confirm_code(&version), deploy_code(AGENT, &version));
    let steps = vec![
        Step {
            number: 1,
            title: "the owner signs in and sees their workspace",
            parts: vec![
                gesture(
                    "the workspace's paper control stream, opened",
                    with(
                        &[
                            "workspace",
                            "open",
                            "--workspace",
                            workspace,
                            "--environment",
                            "paper",
                        ],
                        &target,
                    ),
                    format!("opened ctl:{workspace}"),
                ),
                gesture(
                    "the founder's membership, the owner bundle (the founding grant)",
                    with(
                        &[
                            "workspace",
                            "found",
                            "--workspace",
                            workspace,
                            "--member",
                            FOUNDER,
                        ],
                        &[&["--method", "oidc"][..], &target].concat(),
                    ),
                    format!("founded ctl:{workspace} for member {FOUNDER}"),
                ),
                Part::Api(ApiCheck {
                    shows: "the cell's API, through the demo's port-forward, refusing a read \
                            of the demo workspace with no session (401)",
                    path: format!("/v1/workspaces/{DEMO_WORKSPACE}/agents"),
                    status: 401,
                }),
                Part::Gap(Gap {
                    what: "the demo owner signs in and reads the workspace through the API",
                    on_main: "the API in the cell (#1554) with the stub issuer's sidecar wired \
                              (#1572), `mandate identity link` (#1529), `cell demo`'s own sign-in, \
                              and web/'s sign-in to a dev cell (#1588)",
                    waits: vec![
                        SI_SESSION,
                        wait(
                            "E2E",
                            "E21-54",
                            None,
                            "this scenario signs in and reads the API's own workspace with the \
                             session, a partial step while the gestures use a fresh one",
                        ),
                    ],
                }),
            ],
        },
        Step {
            number: 2,
            title: "the owner deploys an agent with a confirmed mandate envelope",
            parts: vec![
                gesture(
                    "SPY's instrument snapshot, registered",
                    owner(&[
                        "config",
                        "register",
                        "--kind",
                        "instrument_snapshot",
                        &instrument_in,
                    ]),
                    "registered as event".into(),
                ),
                gesture(
                    "the moving-average crossover model, registered",
                    owner(&["model", "register", "quant.ma_crossover", "1.0.0"]),
                    "registered as event".into(),
                ),
                gesture(
                    "the envelope's version, created",
                    owner(&["version", "create", &mandate_in]),
                    format!("created {version}"),
                ),
                gesture(
                    "its confirmation code, shown (nothing committed)",
                    owner(&["version", "confirm", &version]),
                    format!("code {confirm}"),
                ),
                gesture(
                    "the envelope, confirmed with the code shown",
                    owner(&["version", "confirm", &version, "--code", &confirm]),
                    "confirmed as event".into(),
                ),
                gesture(
                    "the deployment's code, shown (nothing committed)",
                    owner(&["agent", "deploy", AGENT, &version]),
                    format!("code {deploy}"),
                ),
                gesture(
                    "the agent, deployed with the code shown",
                    owner(&["agent", "deploy", AGENT, &version, "--code", &deploy]),
                    "deployed as event".into(),
                ),
                Part::Gap(Gap {
                    what: "the same gestures from web/",
                    on_main: "the API's version, confirm and deploy routes (#1552) in the cell \
                              (#1554), web/'s set-up (#1557) and its sign-in to a dev cell \
                              (#1588)",
                    waits: vec![SI_SESSION, W2_WEB],
                }),
                Part::Gap(Gap {
                    what: "the agent's runtime started from AgentDeployed",
                    on_main: "the deployment manager in the cell (#1582), the runtime's image \
                              (#1560), and the runtime reading its deployment and mandate from \
                              the manager (#1576, #1577)",
                    waits: vec![D1_RUNTIME],
                }),
            ],
        },
        Step {
            number: 3,
            title: "the research loop reads news and filings and asks the model",
            parts: vec![Part::Gap(Gap {
                what: "a research run in the agent's runtime, through mandate-gateway's \
                       recorded fake",
                on_main: "the runtime and its image (#1521, #1560), the research driver in the \
                          runtime behind a deny-all default (#1575), the loop through the \
                          gateway's recorded fake (#1520), and E19-5's invalidation (#1559)",
                waits: vec![
                    D1_RUNTIME,
                    wait(
                        "I2",
                        "E19-2",
                        None,
                        "a deployment's research configured, its context folded from the \
                         account and meter streams (DEC-2208, claim #1531)",
                    ),
                    wait(
                        "AH",
                        "E19-13",
                        None,
                        "the agentic loop's read and think tools (ADR-0005, claim #1534)",
                    ),
                ],
            })],
        },
        Step {
            number: 4,
            title: "deterministic code sizes the order, the gate and autonomy rules ask, the \
                    owner approves, the paper order fills and the stop goes out",
            parts: vec![
                Part::Gap(Gap {
                    what: "the runtime sizes the opening and the gate and autonomy rules ask",
                    on_main: "the runtime and its image (#1521, #1560), on the deny-all wiring \
                              that proposes nothing (DEC-1013 item 2)",
                    waits: vec![
                        D1_RUNTIME,
                        wait(
                            "P5",
                            "E21-2",
                            None,
                            "the runtime's production ports, the post-R0 hook (DEC-968 item 6)",
                        ),
                    ],
                }),
                gesture(
                    "the owner's inbox, read (empty until the runtime asks)",
                    owner(&["approvals", "list", "--agent", AGENT]),
                    "no approvals".into(),
                ),
                Part::Gap(Gap {
                    what: "the owner approves the ask in web/",
                    on_main: "the approval answer in web/ and the API's responses route in the \
                              cell (#1554); `mandate approvals approve` is the CLI's path, with \
                              nothing to approve until the runtime asks",
                    waits: vec![
                        wait(
                            "L1",
                            "E9-4",
                            Some(1592),
                            "step-up the API verifies before a grant (DEC-1007 item 3, DEC-2103), \
                             and passkey enrolment, without which every grant answers \
                             `step_up_missing`",
                        ),
                        wait(
                            "W2",
                            "E11-9",
                            None,
                            "web/'s half of step-up on the approval answer (claim #1443)",
                        ),
                    ],
                }),
                Part::Gap(Gap {
                    what: "the paper fill and the protective stop on a fixture broker",
                    on_main: "the executor crate beside the runtime and its image (#1521, \
                              #1560), with the runtime asking nothing yet",
                    waits: vec![X_EXECUTOR],
                }),
            ],
        },
        Step {
            number: 5,
            title: "a backtest of the same mandate version is shown",
            parts: vec![Part::Gap(Gap {
                what: "the backtest of this version, its report read through the API and \
                       shown in web/",
                on_main: "the runner in the cell's image and `mandate backtest record` (#1581, \
                          DEC-1144), the API's backtest read (#1555), and web/'s report screen \
                          (#1549)",
                waits: vec![
                    wait(
                        "E2E",
                        "E21-54",
                        None,
                        "this scenario runs the runner and records it in the gestures' pod",
                    ),
                    SI_SESSION,
                ],
            })],
        },
        Step {
            number: 6,
            title: "the kill switch from the web app stops the agent",
            parts: vec![
                Part::Gap(Gap {
                    what: "the kill switch from web/, recorded through the API",
                    on_main: "POST /v1/workspaces/{ws}/kill-switch in the cell (#1457, #1554), \
                              web/'s kill switch sent through the API, and its sign-in to a dev \
                              cell (#1588)",
                    waits: vec![SI_SESSION, W2_WEB],
                }),
                Part::Gap(Gap {
                    what: "the kill switch through the CLI, at agent scope",
                    on_main: "`mandate agent kill` at agent, connection and workspace scope \
                              (#1537, DEC-2000)",
                    waits: vec![wait(
                        "E2E",
                        "E21-54",
                        None,
                        "this scenario runs it as a gesture and the closing verify covers it",
                    )],
                }),
                Part::Gap(Gap {
                    what: "the agent's processes stop on it",
                    on_main: "the runtime reads the kill switch first (#1521, image #1560)",
                    waits: vec![D1_RUNTIME, X_EXECUTOR],
                }),
            ],
        },
        Step {
            number: 7,
            title: "the record: the workspace's control stream, exported and verified \
                    (journal spec §11)",
            parts: vec![
                gesture(
                    "the control stream, exported",
                    with(
                        &["journal", "export", &stream, SEGMENT],
                        &["--journal", JOURNAL],
                    ),
                    format!("exported: stream {stream}"),
                ),
                gesture(
                    "the export, verified against the artifacts its records name",
                    with(&["journal", "verify", SEGMENT], &["--store", STORE]),
                    format!("result: verified, stream {stream}"),
                ),
            ],
        },
    ];
    steps
}

/// Every gesture of `steps`, in the order the scenario runs them.
pub fn gestures(steps: &[Step]) -> Vec<&Gesture> {
    steps
        .iter()
        .flat_map(|s| &s.parts)
        .filter_map(|p| match p {
            Part::Gesture(g) => Some(g),
            Part::Api(_) | Part::Gap(_) => None,
        })
        .collect()
}

/// The gestures' Job: the chart's own `control` run Job for the first gesture, its container
/// repeated for each gesture in order as init containers after the chart's own, the last one as
/// the pod's container, with the demo's files mounted read-only from a ConfigMap at [`INPUTS`].
/// One pod, so the artifact store `version create` writes is the one `version confirm` and
/// `agent deploy` read, and the one the closing `journal verify` checks every record's artifact
/// against.
pub fn gestures_job(
    rendered: Vec<Value>,
    all: &[&Gesture],
    configmap: &str,
    files: &[(&str, String)],
) -> Result<Vec<Value>> {
    let mut objects = rendered;
    let job = objects
        .iter_mut()
        .find(|o| o["kind"] == "Job")
        .context("the run Job did not render")?;
    let labels = job["metadata"]["labels"].clone();
    job["spec"]["activeDeadlineSeconds"] = json!(DEADLINE_S);
    let spec = &mut job["spec"]["template"]["spec"];
    let template = spec["containers"]
        .as_array()
        .and_then(|c| c.first())
        .cloned()
        .context("the run Job has no container")?;
    let mut containers = Vec::new();
    for gesture in all {
        let mut container = template.clone();
        container["name"] = json!(gesture.container);
        container["args"] = json!(gesture.args);
        container["volumeMounts"]
            .as_array_mut()
            .context("the run container has no mounts")?
            .push(json!({"name": "in", "mountPath": INPUTS, "readOnly": true}));
        containers.push(container);
    }
    let last = containers.pop().context("the scenario has no gesture")?;
    spec["initContainers"]
        .as_array_mut()
        .context("the run Job has no init containers")?
        .extend(containers);
    spec["containers"] = json!([last]);
    spec["volumes"]
        .as_array_mut()
        .context("the run Job has no volumes")?
        .push(json!({"name": "in", "configMap": {"name": configmap}}));
    let data: serde_json::Map<String, Value> = files
        .iter()
        .map(|(name, content)| (name.to_string(), json!(content)))
        .collect();
    objects.push(json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {"name": configmap, "namespace": NAMESPACE, "labels": labels},
        "data": data,
    }));
    Ok(objects)
}

/// The lines `container` printed, from a Job's logs as `kubectl logs --prefix` or the per-container
/// fallback writes them.
pub fn lines_of<'a>(logs: &'a str, container: &str) -> Vec<&'a str> {
    let suffix = format!("/{container}] ");
    let bare = format!("[{container}] ");
    logs.lines()
        .filter_map(|line| {
            let prefixed = line
                .strip_prefix("[pod/")
                .and_then(|rest| rest.split_once("] "))
                .filter(|(pod, _)| format!("{pod}] ").ends_with(&suffix))
                .map(|(_, said)| said);
            prefixed.or_else(|| line.strip_prefix(bare.as_str()))
        })
        .collect()
}

/// Each container's exit code in a `kubectl get pods -o json` list, init containers included; a
/// container that has not terminated has none.
pub fn exit_codes(pods: &Value) -> BTreeMap<String, i64> {
    let mut codes = BTreeMap::new();
    for pod in pods["items"].as_array().into_iter().flatten() {
        for key in ["initContainerStatuses", "containerStatuses"] {
            for status in pod["status"][key].as_array().into_iter().flatten() {
                let name = status["name"].as_str();
                let code = status["state"]["terminated"]["exitCode"].as_i64();
                if let (Some(name), Some(code)) = (name, code) {
                    codes.insert(name.to_string(), code);
                }
            }
        }
    }
    codes
}

/// `cargo xtask cell demo --scenario <name>`.
pub fn run(name: &str) -> Result<()> {
    if !SCENARIOS.contains(&name) {
        bail!(
            "no scenario `{name}`; the scenarios are {}",
            SCENARIOS.join(", ")
        );
    }
    if let Some(reason) = tools_missing() {
        bail!("the scenario needs docker, k3d and helm: {reason} (deploy/cell/README.md)");
    }
    up()?;
    let id = run_id()?;
    let workspace = format!("demo{id}");
    let mandate = fs::read(MANDATE_FILE).with_context(|| format!("reading {MANDATE_FILE}"))?;
    let instrument = fs::read_to_string(INSTRUMENT_FILE)
        .with_context(|| format!("reading {INSTRUMENT_FILE}"))?;
    let steps = plan(&workspace, &mandate);
    let all = gestures(&steps);
    let first = all.first().context("the scenario has no gesture")?;
    let rendered = render(
        &[
            format!("run.id=\"{id}\""),
            format!("run.args={}", json!(first.args)),
        ],
        Some("templates/run.yaml"),
    )?;
    let files = [
        (
            "mandate.json",
            String::from_utf8(mandate).context("the mandate is not UTF-8")?,
        ),
        ("instrument.json", instrument),
    ];
    let objects = gestures_job(rendered, &all, &format!("demo-in-{id}"), &files)?;
    eprintln!("==> the demo, end to end (paper and demo data only; local-cell results)");
    eprintln!("    workspace {workspace}, agent {AGENT}");
    let job = objects
        .iter()
        .find(|o| o["kind"] == "Job")
        .and_then(|o| o["metadata"]["name"].as_str())
        .context("the gestures' Job has no name")?
        .to_string();
    let (succeeded, logs) = run_job(objects)?;
    let pods = kubectl(&[
        "get",
        "pods",
        "--namespace",
        NAMESPACE,
        "--selector",
        &format!("job-name={job}"),
        "-o",
        "json",
    ])?;
    let codes = exit_codes(&serde_json::from_str(&pods).context("the gestures' pods")?);
    let mut failed = None;
    let mut gaps = 0;
    for step in &steps {
        let waiting = lanes(step);
        let on = if waiting.is_empty() {
            String::new()
        } else {
            format!(" (waits on {})", waiting.join(", "))
        };
        eprintln!(
            "  step {}  {:<8} {}{on}",
            step.number,
            readiness(step).word(),
            step.title
        );
        for part in &step.parts {
            match part {
                Part::Gesture(g) => {
                    let lines = lines_of(&logs, &g.container);
                    let said = lines.iter().any(|l| l.contains(&g.expect))
                        && codes.get(&g.container) == Some(&0);
                    let last = lines
                        .iter()
                        .find(|l| l.contains(&g.expect))
                        .or(lines.last())
                        .copied()
                        .unwrap_or("(no output)");
                    if failed.is_some() {
                        eprintln!("    not run  {}", g.shows);
                    } else if said {
                        eprintln!("    ran      {}: {last}", g.shows);
                    } else {
                        eprintln!(
                            "    failed   {}: `mandate {}`: {last}",
                            g.shows,
                            g.args.join(" ")
                        );
                        failed = Some(g.container.clone());
                    }
                }
                Part::Api(check) => {
                    let answer = api_status(&check.path)?;
                    let required = env::var_os(CELL_REQUIRED_ENV).is_some();
                    let (word, said, fails) = api_verdict(&answer, check.status, required);
                    eprintln!("    {word:<8} {}: {said}", check.shows);
                    if fails {
                        failed.get_or_insert_with(|| format!("GET {}: {said}", check.path));
                    }
                }
                Part::Gap(gap) => {
                    gaps += 1;
                    eprintln!("    gap      {}", gap.what);
                    eprintln!("             on main: {}", gap.on_main);
                    for wait in &gap.waits {
                        eprintln!("             waits:   {}: {}", wait.named(), wait.lands);
                    }
                }
            }
        }
    }
    eprintln!("==> what the story waits on, by lane");
    for (lane, waits) in by_lane(&steps) {
        eprintln!("    {lane}");
        for (step, gap, wait) in waits {
            eprintln!(
                "      step {step}  {}: {} ({})",
                gap.what,
                wait.lands,
                wait.named()
            );
        }
    }
    if let Some(container) = failed {
        bail!("the demo's gesture {container} did not do what it shows:\n{logs}");
    }
    if !succeeded {
        bail!("the demo's gestures Job failed:\n{logs}");
    }
    eprintln!(
        "==> {} gestures ran, {gaps} gaps named; kubectl: docker exec {} kubectl --namespace \
         {NAMESPACE} get jobs",
        all.len(),
        NODE
    );
    Ok(())
}

/// The status the cell's API answers `GET path` with through the demo's port-forward, or why it
/// cannot be asked: no API in the process list, a cell that pulled its images and has no API image,
/// or no `kubectl` on this machine to forward with.
fn api_status(path: &str) -> Result<Result<u16, String>> {
    let listed = processes()?
        .iter()
        .any(|p| p["name"] == API_PROCESS && p["built"] == true);
    if !listed {
        return Ok(Err("the API is not in the cell's process list".into()));
    }
    if api_image_tag()?.is_none() {
        return Ok(Err(
            "the cell pulled its images and no API image is published yet".into(),
        ));
    }
    if output("kubectl", &["version", "--client"]).is_err() {
        return Ok(Err(
            "no kubectl on this machine to forward the API's port with".into(),
        ));
    }
    let forwarded = api_port_forward(stub_image_tag()?.is_some());
    Ok(answer_of(forwarded, || http_status(API_LOCAL_PORT, path)))
}

/// What the API answered, given the port-forward's result and a probe of the forwarded port. The
/// forward bails on a reachable API that answers anything but 401, so its error alone cannot tell
/// an unreachable API from a wrong one: the probe decides. A status it reads is the API's answer,
/// which the verdict fails unless it is the expected one; only a probe that reads none means the
/// API could not be asked.
pub fn answer_of(
    forwarded: Result<()>,
    probe: impl FnOnce() -> Result<u16>,
) -> Result<u16, String> {
    match (forwarded, probe()) {
        (_, Ok(status)) => Ok(status),
        (Err(e), Err(_)) => Err(format!("the port-forward to the API failed: {e:#}")),
        (Ok(()), Err(e)) => Err(format!("the API did not answer: {e:#}")),
    }
}

/// How an API check reads: `ran` only for the status it expects; `failed` for any other status,
/// and for an API that cannot be asked where CI requires the cell; otherwise `not run`. Returns the
/// word, what to print after it, and whether the run fails.
pub fn api_verdict(
    answer: &Result<u16, String>,
    expected: u16,
    required: bool,
) -> (&'static str, String, bool) {
    match answer {
        Ok(status) if *status == expected => ("ran", format!("answered {status}"), false),
        Ok(status) => ("failed", format!("answered {status}, not {expected}"), true),
        Err(why) if required => ("failed", why.clone(), true),
        Err(why) => ("not run", why.clone(), false),
    }
}

/// `cargo xtask ci cell-demo`: the scenario on the CI runner's cell. Skipped without docker, k3d
/// and helm unless CI requires it.
pub fn ci() -> Result<()> {
    if let Some(reason) = tools_missing() {
        if std::env::var_os(CELL_REQUIRED_ENV).is_some() {
            bail!("{CELL_REQUIRED_ENV} is set but {reason}");
        }
        eprintln!("    cell-demo: {reason}; skipping (see deploy/cell/README.md)");
        return Ok(());
    }
    run("demo")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mandate() -> Vec<u8> {
        fs::read(format!("../{MANDATE_FILE}")).unwrap()
    }

    fn parsed(path: &str) -> Value {
        serde_json::from_slice(&fs::read(format!("../{path}")).unwrap()).unwrap()
    }

    /// A version is the SHA-256 of the file only when the file is its own canonical form: keys
    /// sorted, no whitespace. serde_json's map is ordered, and the files hold no number but small
    /// integers and no string but ASCII, where its compact form is RFC 8785's.
    #[test]
    fn the_demo_files_are_canonical_json() {
        for path in [MANDATE_FILE, INSTRUMENT_FILE] {
            let bytes = fs::read(format!("../{path}")).unwrap();
            let again = serde_json::to_vec(&parsed(path)).unwrap();
            assert_eq!(bytes, again, "{path} is not canonical JSON");
            assert!(bytes.is_ascii(), "{path} holds a non-ASCII byte");
        }
    }

    /// The demo is paper, asks for every opening, and trades only the instrument it registers.
    #[test]
    fn the_demo_mandate_is_paper_and_asks_every_opening() {
        let m = parsed(MANDATE_FILE);
        assert_eq!(m["environment"], "paper");
        assert_eq!(m["autonomy"]["default"], "ask");
        assert_eq!(m["autonomy"]["admission"], "ask");
        assert_eq!(
            m["autonomy"]["rules"],
            json!([]),
            "no rule makes an opening AUTO"
        );
        assert_eq!(
            m["protection"]["enabled"], true,
            "the fill gets its protective stop"
        );
        let instrument = parsed(INSTRUMENT_FILE);
        let pinned = &m["universe"]["pinned_instruments"];
        assert_eq!(pinned.as_array().map(Vec::len), Some(1));
        assert_eq!(pinned[0]["asset_id"], instrument["instrument_id"]);
        assert_eq!(pinned[0]["symbol"], instrument["symbol"]);
    }

    /// DEC-530 item 4's codes for the demo's mandate, as the CLI binary showed them when the
    /// scenario's gestures were run against a local Postgres (and the CLI accepted them typed).
    #[test]
    fn the_codes_are_the_first_eight_hex_digits_of_the_gestures_hash() {
        let version = "sha256:19dd2428a410b41fa00ddc1b3d8d15d5775096ed223a20edf123c50adae292e6";
        assert_eq!(confirm_code(version), "e869cb19");
        assert_eq!(deploy_code(AGENT, version), "63fbe39f");
        assert_eq!(code(""), "e3b0c442");
    }

    /// The steps run 1 to 6 in order; a shown code precedes the gesture that types it, and the
    /// typed code is the one shown; every gesture's container name is unique and a Kubernetes name.
    #[test]
    fn the_plan_runs_the_six_steps_and_types_the_codes_it_shows() {
        let steps = plan("demo1", &mandate());
        let numbers: Vec<u8> = steps.iter().map(|s| s.number).collect();
        assert_eq!(
            numbers,
            [1, 2, 3, 4, 5, 6, 7],
            "the six steps, then the record"
        );
        let all = gestures(&steps);
        let mut names: Vec<&str> = all.iter().map(|g| g.container.as_str()).collect();
        for name in &names {
            assert!(name.len() <= 63, "{name}");
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{name}"
            );
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len(), "container names repeat");
        for (i, g) in all.iter().enumerate() {
            if let Some(at) = g.args.iter().position(|a| a == "--code") {
                let typed = &g.args[at + 1];
                let shown = all[..i].iter().rev().find(|s| s.args[..2] == g.args[..2]);
                assert_eq!(
                    shown.map(|s| s.expect.clone()),
                    Some(format!("code {typed}")),
                    "{}: the typed code was shown first",
                    g.container
                );
            }
        }
        let version = reference(&mandate());
        assert!(
            all.iter().any(|g| g.expect == format!("created {version}")),
            "version create names the file's own version"
        );
    }

    /// Every gesture states the workspace it acts on, or its control stream, never the live
    /// environment, and names the pod's own journal and store; the record is exported last and
    /// verified against that store, so a record's missing artifact fails the run.
    #[test]
    fn every_gesture_is_in_the_demo_workspace_and_never_live() {
        let steps = plan("demo7", &mandate());
        let all = gestures(&steps);
        fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
            let at = args.iter().position(|a| a == flag);
            at.and_then(|i| args.get(i + 1)).map(String::as_str)
        }
        for g in &all {
            let (export, verify) = (
                g.args[..2] == ["journal", "export"],
                g.args[..2] == ["journal", "verify"],
            );
            let scoped = value(&g.args, "--workspace") == Some("demo7")
                || (export && g.args[2] == "ctl:demo7")
                || verify;
            assert!(scoped, "{:?}", g.args);
            assert!(!g.args.iter().any(|a| a == "live"), "{:?}", g.args);
            if !verify {
                assert_eq!(value(&g.args, "--journal"), Some(JOURNAL), "{:?}", g.args);
            }
            if !export {
                assert_eq!(value(&g.args, "--store"), Some(STORE), "{:?}", g.args);
            }
        }
        let tail: Vec<&[String]> = all[all.len() - 2..].iter().map(|g| &g.args[..2]).collect();
        assert_eq!(tail, [["journal", "export"], ["journal", "verify"]]);
        assert_eq!(all[all.len() - 2].args[3], SEGMENT);
        assert_eq!(all[all.len() - 1].args[2], SEGMENT);
    }

    /// The API is checked only for what it serves without a session: step 1 asks for the demo
    /// workspace's agents and must be refused with the one 401, and no check expects a success the
    /// cell cannot give until a session exists.
    #[test]
    fn the_api_is_checked_only_for_its_refusal_without_a_session() {
        let steps = plan("demo1", &mandate());
        let checks: Vec<(u8, &ApiCheck)> = steps
            .iter()
            .flat_map(|s| s.parts.iter().map(move |p| (s.number, p)))
            .filter_map(|(n, p)| match p {
                Part::Api(c) => Some((n, c)),
                _ => None,
            })
            .collect();
        assert_eq!(checks.len(), 1, "one API check");
        let (step, check) = checks[0];
        assert_eq!(step, 1);
        assert_eq!(check.status, 401);
        assert_eq!(
            check.path,
            format!("/v1/workspaces/{DEMO_WORKSPACE}/agents")
        );
    }

    /// Only the expected status reads as run; another status always fails the run, and an API
    /// that cannot be asked fails it only where CI requires the cell.
    #[test]
    fn an_api_check_runs_only_on_its_status_and_fails_where_the_cell_is_required() {
        let unreachable: Result<u16, String> = Err("no kubectl".into());
        for required in [false, true] {
            assert_eq!(api_verdict(&Ok(401), 401, required).0, "ran");
            assert!(
                !api_verdict(&Ok(401), 401, required).2,
                "a 401 never fails the run"
            );
            for status in [200, 403, 404, 500] {
                let (word, _, fails) = api_verdict(&Ok(status), 401, required);
                assert_eq!(
                    (word, fails),
                    ("failed", true),
                    "{status}, required {required}"
                );
            }
        }
        assert_eq!(
            api_verdict(&unreachable, 401, false),
            ("not run", "no kubectl".into(), false)
        );
        assert_eq!(
            api_verdict(&unreachable, 401, true),
            ("failed", "no kubectl".into(), true)
        );
    }

    /// A wrong status from a reachable API is an answer, even when the port-forward bailed on it,
    /// so the verdict fails it; only a probe that reads nothing makes the API unreachable.
    #[test]
    fn a_reachable_api_that_answers_wrongly_is_an_answer_not_unreachable() {
        let bailed = || Err(anyhow::anyhow!("answered 404, not the one 401"));
        assert_eq!(answer_of(bailed(), || Ok(404)), Ok(404));
        assert_eq!(
            api_verdict(&answer_of(bailed(), || Ok(404)), 401, false).0,
            "failed"
        );
        assert_eq!(answer_of(Ok(()), || Ok(401)), Ok(401));
        let refused = || Err(anyhow::anyhow!("connection refused"));
        assert!(answer_of(bailed(), refused).is_err(), "nothing answered");
        assert!(answer_of(Ok(()), refused).is_err(), "nothing answered");
        assert_eq!(
            api_verdict(&answer_of(Ok(()), refused), 401, false).0,
            "not run"
        );
    }

    /// Whether `text` cites a PR, `#` and digits, as a word.
    fn cites_a_pr(text: &str) -> bool {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .any(|w| {
                w.strip_prefix('#')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
    }

    /// Whether `story` is a backlog story, `E<epic>-<n>`, with its own entry in the backlog.
    fn a_backlog_story(story: &str, backlog: &str) -> bool {
        let shaped = story
            .strip_prefix('E')
            .and_then(|r| r.split_once('-'))
            .is_some_and(|(e, n)| {
                !e.is_empty()
                    && !n.is_empty()
                    && e.bytes().all(|b| b.is_ascii_digit())
                    && n.bytes().all(|b| b.is_ascii_digit())
            });
        let entry = format!("- **{story}");
        shaped
            && backlog.lines().any(|l| {
                l.trim_start()
                    .strip_prefix(&entry)
                    .is_some_and(|rest| rest.starts_with([' ', '*', '(', ':']))
            })
    }

    /// A gap is named, never faked (DEC-2004): it says what of it is on `main`, citing a PR, and
    /// waits on at least one lane, each wait naming a lane, a story the backlog holds, and what
    /// that lane still lands; a PR, when named, is a real number.
    #[test]
    fn every_gap_waits_on_a_named_lane_and_its_backlog_story() {
        let backlog = fs::read_to_string("../docs/project/06-backlog-v1.md").unwrap();
        let steps = plan("demo1", &mandate());
        for step in &steps {
            for part in &step.parts {
                let Part::Gap(gap) = part else { continue };
                let at = format!("step {}: {}", step.number, gap.what);
                assert!(cites_a_pr(gap.on_main), "{at}: on main cites no PR");
                assert!(!gap.waits.is_empty(), "{at}: waits on nobody");
                for wait in &gap.waits {
                    assert!(
                        (1..=4).contains(&wait.lane.len())
                            && wait
                                .lane
                                .bytes()
                                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()),
                        "{at}: lane {:?}",
                        wait.lane
                    );
                    assert!(
                        a_backlog_story(wait.story, &backlog),
                        "{at}: {} is not a backlog story",
                        wait.story
                    );
                    assert!(!wait.lands.is_empty(), "{at}: {} lands nothing", wait.lane);
                    assert_ne!(wait.pr, Some(0), "{at}");
                }
            }
        }
        assert!(
            !cites_a_pr("no PR, E21-54"),
            "the oracle refuses a story alone"
        );
        assert!(
            !a_backlog_story("E99-999", &backlog),
            "the oracle refuses a made-up story"
        );
        assert!(
            !a_backlog_story("E21-5", "- **E21-54 (Proposed)"),
            "a prefix is not a story"
        );
    }

    /// Which steps run today: the gestures on `main` make steps 1, 2 and 4 partial, steps 3, 5
    /// and 6 wait wholly on other lanes, and the record runs whole.
    #[test]
    fn each_step_reads_ready_partial_or_skipped_from_its_parts() {
        let steps = plan("demo1", &mandate());
        let read: Vec<Readiness> = steps.iter().map(readiness).collect();
        use Readiness::{Partial, Ready, Skipped};
        assert_eq!(
            read,
            [Partial, Partial, Skipped, Partial, Skipped, Skipped, Ready]
        );
        let step = |parts: Vec<Part>| Step {
            number: 9,
            title: "t",
            parts,
        };
        let gap = |lanes: &[&'static str]| {
            Part::Gap(Gap {
                what: "w",
                on_main: "#1",
                waits: lanes.iter().map(|l| wait(l, "E1-1", None, "x")).collect(),
            })
        };
        let check = || {
            Part::Api(ApiCheck {
                shows: "s",
                path: "/".into(),
                status: 401,
            })
        };
        assert_eq!(readiness(&step(vec![check()])), Ready);
        assert_eq!(readiness(&step(vec![check(), gap(&["A"])])), Partial);
        assert_eq!(readiness(&step(vec![gap(&["A"]), gap(&["B"])])), Skipped);
        assert_eq!(
            lanes(&step(vec![gap(&["B", "A"]), check(), gap(&["A", "C"])])),
            ["B", "A", "C"],
            "each lane once, in the order first named"
        );
        assert!(lanes(&step(vec![check()])).is_empty());
    }

    /// The closing summary lists every wait of every gap once, under its own lane, with the step it
    /// holds back.
    #[test]
    fn the_summary_lists_every_wait_once_under_its_lane() {
        let steps = plan("demo1", &mandate());
        let grouped = by_lane(&steps);
        let mut counted = BTreeMap::<&str, usize>::new();
        for step in &steps {
            for part in &step.parts {
                if let Part::Gap(gap) = part {
                    for wait in &gap.waits {
                        *counted.entry(wait.lane).or_default() += 1;
                        let listed = &grouped[wait.lane];
                        assert!(
                            listed.iter().any(|(n, g, w)| *n == step.number
                                && std::ptr::eq(*g, gap)
                                && std::ptr::eq(*w, wait)),
                            "step {}: {} under {}",
                            step.number,
                            gap.what,
                            wait.lane
                        );
                    }
                }
            }
        }
        let sizes: BTreeMap<&str, usize> = grouped.iter().map(|(l, w)| (*l, w.len())).collect();
        assert_eq!(sizes, counted);
        assert_eq!(
            wait("SI", "E21-52", Some(1591), "x").named(),
            "SI (E21-52, #1591)"
        );
        assert_eq!(wait("D1", "E21-33", None, "x").named(), "D1 (E21-33)");
    }

    fn rendered() -> Vec<Value> {
        vec![json!({
            "kind": "Job",
            "metadata": {"name": "control-1", "labels": {"app": "x"}},
            "spec": {"activeDeadlineSeconds": 120, "template": {"spec": {
                "initContainers": [{"name": "egress-gate"}, {"name": "credentials"}],
                "containers": [{"name": "control", "args": ["workspace"], "volumeMounts": [{"name": "work"}]}],
                "volumes": [{"name": "work"}]
            }}}
        })]
    }

    /// The gestures run in one pod, in order, after the chart's own init containers: the egress
    /// gate first, so no gesture runs before the pod's egress is filtered.
    #[test]
    fn the_job_runs_every_gesture_in_order_after_the_egress_gate() {
        let steps = plan("demo1", &mandate());
        let all = gestures(&steps);
        let files = [("mandate.json", "{}".to_string())];
        let objects = gestures_job(rendered(), &all, "demo-in-1", &files).unwrap();
        let spec = &objects[0]["spec"]["template"]["spec"];
        let names: Vec<&str> = spec["initContainers"]
            .as_array()
            .unwrap()
            .iter()
            .chain(spec["containers"].as_array().unwrap())
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        let mut expected = vec!["egress-gate", "credentials"];
        expected.extend(all.iter().map(|g| g.container.as_str()));
        assert_eq!(names, expected);
        let last = &spec["containers"][0];
        assert_eq!(
            last["args"],
            json!(all[all.len() - 1].args),
            "each gesture's own args"
        );
        assert_eq!(
            last["volumeMounts"][1],
            json!({"name": "in", "mountPath": INPUTS, "readOnly": true})
        );
        assert_eq!(
            objects[0]["spec"]["activeDeadlineSeconds"],
            json!(DEADLINE_S)
        );
        assert_eq!(objects[1]["kind"], "ConfigMap");
        assert_eq!(objects[1]["metadata"]["labels"], json!({"app": "x"}));
        assert_eq!(
            spec["volumes"][1],
            json!({"name": "in", "configMap": {"name": "demo-in-1"}})
        );
    }

    #[test]
    fn a_containers_lines_are_read_from_either_log_form() {
        let logs = "[pod/j-x/g01-workspace-open] opened ctl:demo1 as event A at seq 1\n\
                    [pod/j-x/g02-workspace-found] founded\n\
                    [g01-workspace-open] again\n\
                    not a line [pod/j-x/g01-workspace-open] inside\n";
        assert_eq!(
            lines_of(logs, "g01-workspace-open"),
            ["opened ctl:demo1 as event A at seq 1", "again"]
        );
        assert!(lines_of(logs, "g03-config-register").is_empty());
        let echoed = "[pod/j-x/g02-workspace-found] said /g01-workspace-open] this\n";
        assert!(
            lines_of(echoed, "g01-workspace-open").is_empty(),
            "another container's line"
        );
    }

    /// Only a terminated container has a code; init containers count, and a running one does not.
    #[test]
    fn exit_codes_come_from_terminated_containers_only() {
        let pods = json!({"items": [{"status": {
            "initContainerStatuses": [
                {"name": "egress-gate", "state": {"terminated": {"exitCode": 0}}},
                {"name": "g01-workspace-open", "state": {"terminated": {"exitCode": 1}}}
            ],
            "containerStatuses": [{"name": "g10-approvals-list", "state": {"waiting": {}}}]
        }}]});
        let codes = exit_codes(&pods);
        assert_eq!(codes.get("egress-gate"), Some(&0));
        assert_eq!(codes.get("g01-workspace-open"), Some(&1));
        assert_eq!(codes.get("g10-approvals-list"), None);
    }
}
