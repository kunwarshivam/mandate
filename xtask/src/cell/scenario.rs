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
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    API_LOCAL_PORT, API_PROCESS, CELL_REQUIRED_ENV, DEMO_OWNER, DEPLOY_MANAGER_PROCESS, NAMESPACE,
    NODE, WORKSPACE_CHART, api_image_tag, api_port_forward, helm, http, kubectl, open_session,
    processes, render, reply_body, run_id, run_job, session_cookie, signed_in, stub_image_tag,
    stub_token, tools_missing, up, web,
};
use crate::output;

/// The scenarios `--scenario` takes.
pub const SCENARIOS: &[&str] = &["demo"];
/// The demo's mandate, its instrument snapshot, and where each is mounted in the gestures' pod.
/// Both are stored as canonical JSON (journal spec §4), so a version is the SHA-256 of the file.
pub const MANDATE_FILE: &str = "deploy/cell/demo/mandate.json";
pub const INSTRUMENT_FILE: &str = "deploy/cell/demo/instrument.json";
/// [`WEB_AGENT`]'s own envelope and its instrument's snapshot, as canonical JSON: the demo's
/// envelope on QQQ, since V-006 refuses a second agent on the account an instrument the demo agent
/// already pins (DEC-2036).
pub const WEB_MANDATE_FILE: &str = "deploy/cell/demo/web-mandate.json";
pub const WEB_INSTRUMENT_FILE: &str = "deploy/cell/demo/web-instrument.json";
/// The demo's research agent's envelope, as canonical JSON, and the recording its model registers
/// from: an unpinned universe of one instrument, a research envelope, and `llm.research_agent`
/// admitting instruments, pinned to the hash `model register --entry` recomputes from the demo
/// bundle's entry (DEC-2246 item 4, DEC-2247).
pub const RESEARCH_MANDATE_FILE: &str = "deploy/cell/demo/research-mandate.json";
pub const RESEARCH_RECORDING_FILE: &str = "deploy/cell/demo/research/research.json";
const INPUTS: &str = "/in";
/// The journal and the artifact store every gesture names: the `control` process's DSN, expanded
/// by Kubernetes from its environment, and the workspace's shared store at the chart's path, which
/// the control runs write and the API server reads, so step 5's signed-in read finds the report
/// the runner stored. The segment stays on the pod's own `/work`, which all its containers share.
const JOURNAL: &str = "$(MANDATE_JOURNAL_DSN)";
const STORE: &str = "/var/lib/mandate/artifacts";
const SEGMENT: &str = "/work/segment.jsonl";
/// The founding member: the demo owner, the member the stub issuer's `demo-owner` subject is
/// linked to, so the owner who signs in is the one the run's workspace was founded for (DEC-2010).
/// An opaque ULID (identity spec §3.1), never a name; the CLI's owner gestures take the lower-case
/// form, which is all `--user` accepts (DEC-527 item 3).
pub const FOUNDER: &str = DEMO_OWNER;
/// The demo's agent.
pub const AGENT: &str = "demo-spy";
/// A second agent, on its own envelope ([`WEB_MANDATE_FILE`]), deployed for `web/`'s kill switch:
/// the CLI stops [`AGENT`] inside the Job, and `web/` offers nothing to stop on a stopped agent, so
/// the browser presses this one's (lane W2's D10).
pub const WEB_AGENT: &str = "demo-web";
/// The demo's research agent, deployed alone in [`research_workspace_of`] the run's workspace, so
/// its research is the only agent on its control stream (DEC-2214 item 1, DEC-2247 item 2).
pub const RESEARCH_AGENT: &str = "demo-research";
/// How long the gestures' Job may run: the egress gate, the credentials copy and every gesture,
/// one after another. Under the 300 s `run_job` polls for, so the Job ends, and the report is
/// written, before the polling gives up.
const DEADLINE_S: u64 = 240;

/// The research agent's workspace for the run's `workspace`: the same owner's second workspace,
/// `<workspace>r` (DEC-2247 item 2).
pub fn research_workspace_of(workspace: &str) -> String {
    format!("{workspace}r")
}

/// The run's workspace (DEC-2010): a ULID, since the API hosts only a ULID, fresh for each run id
/// because a confirmed version refuses a second confirmation: `01KDEM`, the run id's ten digits,
/// then a fixed tail that no other demo id uses.
pub fn workspace_of(id: &str) -> String {
    format!("01KDEM{id:0>10.10}000000000D")
}

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
    Web(WebCheck),
    Gap(Gap),
}

/// A gesture in `web/` against the cell's API, run after the gestures' Job (lane W2's
/// `kill_switch_from_web`), and the signed-in read that proves it reached the run's control
/// stream: `GET commands/{id}` must find that command on `ctl:{workspace}`, accepted.
#[derive(Debug)]
pub struct WebCheck {
    pub shows: &'static str,
    pub agent: &'static str,
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

/// The waits several gaps share: the agent's runtime started by the deployment manager, web/
/// driven against the cell, and the executor in the cell.
const X_CONNECT: Wait = Wait {
    lane: "X",
    story: "E21-30",
    pr: None,
    lands: "the executor's connect check, so `connection record` can record the demo's paper \
            connection and its deployment is written at version 2, which the manager starts; \
            until then the deployment is version 1 and starts nothing (DEC-1171)",
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
    pub ask: Ask,
    pub body: Body,
}

/// How an [`ApiCheck`] asks: a `GET` of its path with no session; the demo owner's sign-in, a
/// token from the stub issuer traded at `POST /v1/sessions`, which also needs the session cookie;
/// or a `GET` of its path with the session that sign-in opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    NoSession,
    SignIn,
    Session,
}

/// The phases of a command the journal accepted: `recorded` until a process takes it, then
/// `taken`, `applied` or `ended`; never `refused` (the workspace API's command status).
const COMMAND_PHASES: [&str; 4] = ["recorded", "taken", "applied", "ended"];

/// Set where a job must run the web gesture (CT's `cell-demo-web`): there a machine without
/// `web/`'s tools fails the run, as [`CELL_REQUIRED_ENV`] makes a missing cell fail it.
pub const WEB_REQUIRED_ENV: &str = "MANDATE_DEMO_WEB_REQUIRED";

/// What the answer's JSON body must hold besides its status: nothing, an `agents` list naming an
/// agent, string members at JSON pointers, or a command, by id, read from a stream in an accepted
/// phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Any,
    Lists(&'static str),
    Members(Vec<(&'static str, String)>),
    Command { id: String, stream: String },
}

impl Body {
    /// Why `body` does not hold what this asks, or `None` when it does.
    pub fn missing(&self, body: &str) -> Option<String> {
        let value: Value = match self {
            Self::Any => return None,
            Self::Lists(_) | Self::Members(_) | Self::Command { .. } => {
                match serde_json::from_str(body) {
                    Ok(value) => value,
                    Err(_) => return Some("the body is not JSON".into()),
                }
            }
        };
        match self {
            Self::Any => None,
            Self::Lists(agent) => {
                let listed = value["agents"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x["agent_id"] == *agent));
                (!listed).then(|| format!("its agents do not list {agent}"))
            }
            Self::Members(members) => members.iter().find_map(|(pointer, want)| {
                let got = value.pointer(pointer).and_then(Value::as_str);
                (got != Some(want.as_str())).then(|| format!("{pointer} is {got:?}, not {want}"))
            }),
            Self::Command { id, stream } => {
                let phase = value["phase"].as_str().unwrap_or_default();
                let read_from = value["as_of"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x["stream_id"] == *stream));
                if value["command_id"] != *id {
                    Some(format!(
                        "it reads command {}, not {id}",
                        value["command_id"]
                    ))
                } else if !read_from {
                    Some(format!("it is not read from {stream}"))
                } else if !COMMAND_PHASES.contains(&phase) {
                    Some(format!("its phase is {phase:?}"))
                } else {
                    None
                }
            }
        }
    }
}

/// One command in the cell, the text its success prints, and what it shows the demo. A gesture
/// ran only when its container exited 0 and printed `expect`; a shown code is checked this way
/// after the Job, and the CLI itself refuses a typed code that differs from the one it shows. A
/// gesture with a `command` runs that binary of the image instead of its `mandate` entry point.
#[derive(Debug)]
pub struct Gesture {
    pub container: String,
    pub shows: &'static str,
    pub command: Option<&'static str>,
    pub args: Vec<String>,
    pub expect: String,
}

/// The backtest runner in the cell's `mandate` image (#1581), which step 5 runs beside the CLI
/// (DEC-2023).
pub const RUNNER_BIN: &str = "/usr/local/bin/mandate-backtest-runner";

/// The runner's committed demo inputs (DEC-1144 item 3), mounted at [`BACKTEST_IN`].
pub const BACKTEST_INPUTS: &str = "crates/mandate-backtest-runner/tests/fixtures/demo";

/// Where the pod reads the runner's inputs: a directory of their own, since the runner's
/// `instrument.json` is not the CLI's instrument snapshot.
const BACKTEST_IN: &str = "/in/backtest";

/// The demo backtest's id; each run's workspace is fresh, so the id is never another record's.
pub const BACKTEST_ID: &str = "bt-demo-01";

/// The payload and report the runner stores for the demo version and inputs, the committed golden
/// store's (DEC-1144 item 8: the demo's payload reference is fixed).
pub const DEMO_PAYLOAD: &str =
    "sha256:1ab788b2c066c11d949e53929e1ed0332038f481da4d7d6624e6b2027cfdb240";
pub const DEMO_REPORT: &str =
    "sha256:9e672eb266902fc30120100af6bba87f135f0ecfe0a77a00cb4b1d15bcdc4a63";

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

/// The demo's six steps for `workspace`, given the bytes of the demo's mandate, of
/// [`WEB_AGENT`]'s, and of [`RESEARCH_AGENT`]'s.
pub fn plan(
    workspace: &str,
    (mandate, web_mandate, research_mandate): (&[u8], &[u8], &[u8]),
) -> Vec<Step> {
    let version = reference(mandate);
    let web_version = reference(web_mandate);
    let research_version = reference(research_mandate);
    let research_workspace = research_workspace_of(workspace);
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
    let researcher = |args: &[&str]| {
        with(
            args,
            &[
                &["--workspace", research_workspace.as_str(), "--user", &user][..],
                &target,
            ]
            .concat(),
        )
    };
    let stream = format!("ctl:{workspace}");
    let mut index = 0;
    let mut gesture = |shows: &'static str, args: Vec<String>, expect: String| {
        index += 1;
        let (command, args) = match args.split_first() {
            Some((first, rest)) if first == RUNNER_BIN => (Some(RUNNER_BIN), rest.to_vec()),
            _ => (None, args),
        };
        let name = match command {
            Some(_) => format!("runner-{}", args[0]),
            None => args[..2].join("-"),
        };
        Part::Gesture(Gesture {
            container: format!("g{index:02}-{name}"),
            shows,
            command,
            args,
            expect,
        })
    };
    let mandate_in = format!("{INPUTS}/mandate.json");
    let instrument_in = format!("{INPUTS}/instrument.json");
    let (confirm, deploy) = (confirm_code(&version), deploy_code(AGENT, &version));
    let (web_confirm, web_deploy) = (
        confirm_code(&web_version),
        deploy_code(WEB_AGENT, &web_version),
    );
    let (research_confirm, research_deploy) = (
        confirm_code(&research_version),
        deploy_code(RESEARCH_AGENT, &research_version),
    );
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
                            of the run's workspace with no session (401)",
                    path: format!("/v1/workspaces/{workspace}/agents"),
                    status: 401,
                    ask: Ask::NoSession,
                    body: Body::Any,
                }),
                Part::Api(ApiCheck {
                    shows: "the demo owner signing in through the stub issuer: POST \
                            /v1/sessions opens a session (204 and its cookie)",
                    path: "/v1/sessions".into(),
                    status: 204,
                    ask: Ask::SignIn,
                    body: Body::Any,
                }),
                Part::Api(ApiCheck {
                    shows: "the demo owner, signed in, reading the run's workspace through the \
                            API: its agents list the demo's agent (200)",
                    path: format!("/v1/workspaces/{workspace}/agents"),
                    status: 200,
                    ask: Ask::Session,
                    body: Body::Lists(AGENT),
                }),
                Part::Gap(Gap {
                    what: "the same sign-in and read in web/",
                    on_main: "web/'s sign-in to a dev cell (#1588) and the stub issuer (#1591)",
                    waits: vec![W2_WEB],
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
                              (#1554), web/'s set-up (#1557), its sign-in to a dev cell \
                              (#1588) and the stub issuer (#1591)",
                    waits: vec![W2_WEB],
                }),
                Part::Gap(Gap {
                    what: "the agent's runtime started from AgentDeployed",
                    on_main: "the deployment manager in the cell (#1582), starting a runtime \
                              from a version-2 deployment (DEC-1171, E21-33), the runtime's image \
                              (#1560), and the runtime reading its deployment and mandate from \
                              the manager (#1576, #1577)",
                    waits: vec![X_CONNECT],
                }),
            ],
        },
        Step {
            number: 3,
            title: "the research loop reads news and filings and asks the model",
            parts: vec![
                gesture(
                    "the research agent's own paper workspace, opened (DEC-2247 item 2)",
                    with(
                        &[
                            "workspace",
                            "open",
                            "--workspace",
                            &research_workspace,
                            "--environment",
                            "paper",
                        ],
                        &target,
                    ),
                    format!("opened ctl:{research_workspace}"),
                ),
                gesture(
                    "the same founder's membership there",
                    with(
                        &[
                            "workspace",
                            "found",
                            "--workspace",
                            &research_workspace,
                            "--member",
                            FOUNDER,
                        ],
                        &[&["--method", "oidc"][..], &target].concat(),
                    ),
                    format!("founded ctl:{research_workspace} for member {FOUNDER}"),
                ),
                gesture(
                    "the moving-average crossover model, registered there",
                    researcher(&["model", "register", "quant.ma_crossover", "1.0.0"]),
                    "registered as event".into(),
                ),
                gesture(
                    "the research agent's model, registered from the recorded entry, its hash \
                     recomputed (DEC-2247 item 1)",
                    researcher(&[
                        "model",
                        "register",
                        "llm.research_agent",
                        "0.1.0",
                        "--entry",
                        &format!("{INPUTS}/research.json"),
                    ]),
                    "registered as event".into(),
                ),
                gesture(
                    "the research envelope's version, created: unpinned, one instrument, research \
                     on",
                    researcher(&[
                        "version",
                        "create",
                        &format!("{INPUTS}/research-mandate.json"),
                    ]),
                    format!("created {research_version}"),
                ),
                gesture(
                    "its confirmation code, shown (nothing committed)",
                    researcher(&["version", "confirm", &research_version]),
                    format!("code {research_confirm}"),
                ),
                gesture(
                    "the research envelope, confirmed with the code shown (rule 11)",
                    researcher(&[
                        "version",
                        "confirm",
                        &research_version,
                        "--code",
                        &research_confirm,
                    ]),
                    "confirmed as event".into(),
                ),
                gesture(
                    "the research agent's code, shown (nothing committed)",
                    researcher(&["agent", "deploy", RESEARCH_AGENT, &research_version]),
                    format!("code {research_deploy}"),
                ),
                gesture(
                    "the research agent, deployed with the code shown, alone on its control \
                     stream",
                    researcher(&[
                        "agent",
                        "deploy",
                        RESEARCH_AGENT,
                        &research_version,
                        "--code",
                        &research_deploy,
                    ]),
                    "deployed as event".into(),
                ),
                Part::Gap(Gap {
                    what: "a research run in the research agent's runtime, over the recorded \
                           bundle its image carries, through mandate-gateway's recorded fake",
                    on_main: "the runtime opted into research over the bundle (#1757), the \
                              bundle in the interim runtime image (#1769), every managed runtime \
                              pod handed it (#1774), and the research model's registration \
                              (#1779, #1782)",
                    waits: vec![X_CONNECT],
                }),
            ],
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
                        X_CONNECT,
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
                              cell (#1554), the step-up challenge route's spec and schemas \
                              (#1592); `mandate approvals approve` is the CLI's path, with \
                              nothing to approve until the runtime asks",
                    waits: vec![
                        wait(
                            "L1",
                            "E9-4",
                            None,
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
            parts: vec![
                gesture(
                    "the rule set the backtest runs under, registered: the demo run's settings",
                    owner(&[
                        "config",
                        "register",
                        "--kind",
                        "rule_set",
                        &format!("{BACKTEST_IN}/settings.json"),
                    ]),
                    "registered as event".into(),
                ),
                gesture(
                    "the backtest of the confirmed version, run on the demo inputs",
                    with(
                        &[
                            RUNNER_BIN,
                            "run",
                            "--inputs",
                            BACKTEST_IN,
                            "--store",
                            STORE,
                            "--mandate-version",
                            &version,
                            "--backtest-id",
                            BACKTEST_ID,
                        ],
                        &[],
                    ),
                    format!("payload={DEMO_PAYLOAD}"),
                ),
                gesture(
                    "the run, recorded by the backtest runner system after every check",
                    with(
                        &[
                            "backtest",
                            "record",
                            "--workspace",
                            workspace,
                            "--payload",
                            DEMO_PAYLOAD,
                        ],
                        &target,
                    ),
                    format!("report={DEMO_REPORT}"),
                ),
                Part::Api(ApiCheck {
                    shows: "the demo owner, signed in, reading the backtest through the API: \
                            completed, with the report the run stored (200)",
                    path: format!("/v1/workspaces/{workspace}/backtests/{BACKTEST_ID}"),
                    status: 200,
                    ask: Ask::Session,
                    body: Body::Members(vec![
                        ("/backtest/state", "completed".into()),
                        ("/backtest/report_ref", DEMO_REPORT.into()),
                        ("/backtest/mandate_version", version.clone()),
                    ]),
                }),
                Part::Gap(Gap {
                    what: "web/ shows the backtest's report",
                    on_main: "the API's backtest read (#1555), web/'s report screen (#1549), \
                              and the owner's session through the stub issuer (#1591)",
                    waits: vec![W2_WEB],
                }),
            ],
        },
        Step {
            number: 6,
            title: "the kill switch from the web app stops the agent",
            parts: vec![
                gesture(
                    "QQQ's instrument snapshot, registered, for a second agent's envelope",
                    owner(&[
                        "config",
                        "register",
                        "--kind",
                        "instrument_snapshot",
                        &format!("{INPUTS}/web-instrument.json"),
                    ]),
                    "registered as event".into(),
                ),
                gesture(
                    "the second agent's envelope, created: the demo's on QQQ",
                    owner(&["version", "create", &format!("{INPUTS}/web-mandate.json")]),
                    format!("created {web_version}"),
                ),
                gesture(
                    "its confirmation code, shown (nothing committed)",
                    owner(&["version", "confirm", &web_version]),
                    format!("code {web_confirm}"),
                ),
                gesture(
                    "the second envelope, confirmed with the code shown",
                    owner(&["version", "confirm", &web_version, "--code", &web_confirm]),
                    "confirmed as event".into(),
                ),
                gesture(
                    "the second agent's code, shown, for web/'s kill switch",
                    owner(&["agent", "deploy", WEB_AGENT, &web_version]),
                    format!("code {web_deploy}"),
                ),
                gesture(
                    "the second agent, deployed with the code shown",
                    owner(&[
                        "agent",
                        "deploy",
                        WEB_AGENT,
                        &web_version,
                        "--code",
                        &web_deploy,
                    ]),
                    "deployed as event".into(),
                ),
                Part::Web(WebCheck {
                    shows: "the owner's kill switch for the second agent, pressed in web/ and \
                            recorded through the API on the run's control stream",
                    agent: WEB_AGENT,
                }),
                gesture(
                    "the owner's kill switch for the agent, through the CLI (agent scope)",
                    owner(&["agent", "kill", "--agent", AGENT]),
                    format!("kill_switch agent {AGENT} committed as event"),
                ),
                Part::Gap(Gap {
                    what: "the agent's processes stop on it",
                    on_main: "the runtime reads the kill switch first (#1521, image #1560)",
                    waits: vec![X_CONNECT, X_EXECUTOR],
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
            Part::Api(_) | Part::Web(_) | Part::Gap(_) => None,
        })
        .collect()
}

/// The gestures' Job: the chart's own `control` run Job for the first gesture, its container
/// repeated for each gesture in order as init containers after the chart's own, the last one as
/// the pod's container, with the demo's files mounted read-only from a ConfigMap at [`INPUTS`],
/// each at its path in `files`, which may name a directory (a ConfigMap key cannot).
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
    let items: Vec<Value> = files
        .iter()
        .map(|(path, _)| json!({"key": key_of(path), "path": path}))
        .collect();
    let mut containers = Vec::new();
    for gesture in all {
        let mut container = template.clone();
        container["name"] = json!(gesture.container);
        container["args"] = json!(gesture.args);
        if let Some(command) = gesture.command {
            container["command"] = json!([command]);
        }
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
        .push(json!({"name": "in", "configMap": {"name": configmap, "items": items}}));
    let data: serde_json::Map<String, Value> = files
        .iter()
        .map(|(path, content)| (key_of(path), json!(content)))
        .collect();
    objects.push(json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {"name": configmap, "namespace": NAMESPACE, "labels": labels},
        "data": data,
    }));
    Ok(objects)
}

/// The ConfigMap key of the input at `path` under [`INPUTS`]: a key admits no `/`.
fn key_of(path: &str) -> String {
    path.replace('/', ".")
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

/// The runner's five demo inputs under the checkout at `root`, each at its path under
/// [`BACKTEST_IN`] relative to [`INPUTS`].
fn backtest_inputs(root: &Path) -> Result<Vec<(String, String)>> {
    let dir = BACKTEST_IN
        .strip_prefix(&format!("{INPUTS}/"))
        .context("the runner's inputs are not under the pod's")?;
    [
        "bars.json",
        "outputs.json",
        "theses.json",
        "instrument.json",
        "settings.json",
    ]
    .iter()
    .map(|name| {
        let path = root.join(BACKTEST_INPUTS).join(name);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        Ok((format!("{dir}/{name}"), text))
    })
    .collect()
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
    let workspace = workspace_of(&id);
    if api_unaskable()?.is_none() {
        point_api(&workspace)?;
    }
    let mandate = fs::read(MANDATE_FILE).with_context(|| format!("reading {MANDATE_FILE}"))?;
    let instrument = fs::read_to_string(INSTRUMENT_FILE)
        .with_context(|| format!("reading {INSTRUMENT_FILE}"))?;
    let web_mandate =
        fs::read(WEB_MANDATE_FILE).with_context(|| format!("reading {WEB_MANDATE_FILE}"))?;
    let web_instrument = fs::read_to_string(WEB_INSTRUMENT_FILE)
        .with_context(|| format!("reading {WEB_INSTRUMENT_FILE}"))?;
    let research_mandate = fs::read(RESEARCH_MANDATE_FILE)
        .with_context(|| format!("reading {RESEARCH_MANDATE_FILE}"))?;
    let research_recording = fs::read_to_string(RESEARCH_RECORDING_FILE)
        .with_context(|| format!("reading {RESEARCH_RECORDING_FILE}"))?;
    let steps = plan(&workspace, (&mandate, &web_mandate, &research_mandate));
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
            "mandate.json".to_string(),
            String::from_utf8(mandate).context("the mandate is not UTF-8")?,
        ),
        ("instrument.json".into(), instrument),
        (
            "web-mandate.json".into(),
            String::from_utf8(web_mandate).context("the second mandate is not UTF-8")?,
        ),
        ("web-instrument.json".into(), web_instrument),
        (
            "research-mandate.json".into(),
            String::from_utf8(research_mandate).context("the research mandate is not UTF-8")?,
        ),
        ("research.json".into(), research_recording),
    ]
    .into_iter()
    .chain(backtest_inputs(Path::new("."))?)
    .collect::<Vec<_>>();
    let files: Vec<(&str, String)> = files.iter().map(|(p, c)| (p.as_str(), c.clone())).collect();
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
                    let answer = api_answer(check)?;
                    let required = env::var_os(CELL_REQUIRED_ENV).is_some();
                    let (word, said, fails) = api_verdict(&answer, check, required);
                    eprintln!("    {word:<8} {}: {said}", check.shows);
                    if fails {
                        failed.get_or_insert_with(|| format!("GET {}: {said}", check.path));
                    }
                }
                Part::Web(check) => {
                    let outcome = if failed.is_some() {
                        WebOutcome::AfterFailure
                    } else {
                        web_outcome(check, &workspace)?
                    };
                    let required = Required {
                        cell: env::var_os(CELL_REQUIRED_ENV).is_some(),
                        web: env::var_os(WEB_REQUIRED_ENV).is_some(),
                    };
                    let (word, said, fails) = web_verdict(&outcome, check, &workspace, required);
                    eprintln!("    {word:<8} {}: {said}", check.shows);
                    if fails {
                        failed.get_or_insert_with(|| format!("web/ {}: {said}", check.agent));
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

/// Why the cell's API cannot be asked, or `None` when it can: no API in the process list, a cell
/// that pulled its images and has no API image, or no `kubectl` on this machine to forward with.
fn api_unaskable() -> Result<Option<String>> {
    let listed = processes()?
        .iter()
        .any(|p| p["name"] == API_PROCESS && p["built"] == true);
    if !listed {
        return Ok(Some("the API is not in the cell's process list".into()));
    }
    if api_image_tag()?.is_none() {
        return Ok(Some(
            "the cell pulled its images and no API image is published yet".into(),
        ));
    }
    if output("kubectl", &["version", "--client"]).is_err() {
        return Ok(Some(
            "no kubectl on this machine to forward the API's port with".into(),
        ));
    }
    Ok(None)
}

/// Points the cell's API and deployment manager at the run's `workspace` (DEC-2010): a `helm
/// upgrade` of the workspace release that keeps every other value, then a wait until both
/// Deployments have rolled out, so no pod of an earlier run still serves another workspace; and
/// any earlier `kubectl port-forward` to the API is stopped, so the checks never reach an old pod
/// through it (`api_port_forward` would reuse a forward that still answers its 401).
fn point_api(workspace: &str) -> Result<()> {
    let hosted = format!("api.workspace={workspace}");
    helm(&[
        "upgrade",
        "workspace",
        WORKSPACE_CHART,
        "--namespace",
        NAMESPACE,
        "--reuse-values",
        "--set",
        &hosted,
        "--wait",
        "--wait-for-jobs",
        "--timeout",
        "3m",
    ])?;
    for process in [API_PROCESS, DEPLOY_MANAGER_PROCESS] {
        let deployment = format!("deployment/{process}");
        let present = kubectl(&[
            "get",
            &deployment,
            "--namespace",
            NAMESPACE,
            "--ignore-not-found",
            "-o",
            "name",
        ])?;
        if present.trim().is_empty() {
            eprintln!("    {process}: not in this cell, no rollout to wait on");
            continue;
        }
        kubectl(&[
            "rollout",
            "status",
            &deployment,
            "--namespace",
            NAMESPACE,
            "--timeout=120s",
        ])?;
    }
    let _ = output("pkill", &["-f", "port-forward.*deployment/api"]);
    eprintln!("    api: hosting {workspace}, rolled out; an earlier forward to it is stopped");
    Ok(())
}

/// What the cell's API answers `check` with through the demo's port-forward, its status and
/// whole reply, or why it cannot be asked. A sign-in first asks the stub issuer for a token; a
/// stub that gives none reads as an API that could not be asked. A session check whose sign-in sets no
/// cookie answers with that sign-in's reply, which the verdict fails as a wrong answer, never as
/// an API that could not be asked.
fn api_answer(check: &ApiCheck) -> Result<Result<(u16, String), String>> {
    if let Some(why) = api_unaskable()? {
        return Ok(Err(why));
    }
    let forwarded = api_port_forward(stub_image_tag()?.is_some());
    Ok(answer_of(forwarded, || match check.ask {
        Ask::NoSession => http(API_LOCAL_PORT, "GET", &check.path, &[], ""),
        Ask::SignIn => open_session(&stub_token()?),
        Ask::Session => {
            let (status, reply) = open_session(&stub_token()?)?;
            match session_cookie(&reply) {
                Some(cookie) => http(
                    API_LOCAL_PORT,
                    "GET",
                    &check.path,
                    &[("Cookie", &cookie)],
                    "",
                ),
                None => Ok((status, reply)),
            }
        }
    }))
}

/// What the API answered, given the port-forward's result and a probe of the forwarded port. The
/// forward bails on a reachable API that answers anything but 401, so its error alone cannot tell
/// an unreachable API from a wrong one: the probe decides. An answer it reads is the API's, which
/// the verdict fails unless it is the expected one; only a probe that reads none means the API
/// could not be asked.
pub fn answer_of<T>(forwarded: Result<()>, probe: impl FnOnce() -> Result<T>) -> Result<T, String> {
    match (forwarded, probe()) {
        (_, Ok(answer)) => Ok(answer),
        (Err(e), Err(_)) => Err(format!("the port-forward to the API failed: {e:#}")),
        (Ok(()), Err(e)) => Err(format!("the API did not answer: {e:#}")),
    }
}

/// How an API check reads: `ran` only for the status it expects, and for a sign-in only with the
/// session cookie; `failed` for any other answer, and for an API that cannot be asked where CI
/// requires the cell; otherwise `not run`. Returns the word, what to print after it, and whether
/// the run fails.
pub fn api_verdict(
    answer: &Result<(u16, String), String>,
    check: &ApiCheck,
    required: bool,
) -> (&'static str, String, bool) {
    let expected = check.status;
    match answer {
        Ok((status, reply)) if *status == expected => {
            if check.ask == Ask::SignIn && !signed_in(*status, reply) {
                (
                    "failed",
                    format!("answered {status} without the session cookie"),
                    true,
                )
            } else if let Some(why) = check.body.missing(reply_body(reply)) {
                ("failed", format!("answered {status}, but {why}"), true)
            } else {
                ("ran", format!("answered {status}"), false)
            }
        }
        Ok((status, _)) => ("failed", format!("answered {status}, not {expected}"), true),
        Err(why) if required => ("failed", why.clone(), true),
        Err(why) => ("not run", why.clone(), false),
    }
}

/// What became of a [`WebCheck`]: an earlier part failed, so it was not tried; the API could not
/// be asked; this machine lacks `web/`'s tools; the gesture failed; or it recorded a command,
/// with the agent and workspace it acted on and the signed-in read of that command.
#[derive(Debug)]
pub enum WebOutcome {
    AfterFailure,
    Unaskable(String),
    NoTools(String),
    Failed(String),
    Recorded {
        agent: String,
        workspace: String,
        command: String,
        read: Result<(u16, String), String>,
    },
}

/// Runs `check`'s gesture against the cell's API for `workspace`, then reads its command back
/// with the owner's session.
fn web_outcome(check: &WebCheck, workspace: &str) -> Result<WebOutcome> {
    if let Some(why) = api_unaskable()? {
        return Ok(WebOutcome::Unaskable(why));
    }
    if let Some(why) = web::unavailable()? {
        return Ok(WebOutcome::NoTools(why));
    }
    let forwarded = api_port_forward(stub_image_tag()?.is_some());
    if let Err(why) = forwarded {
        return Ok(WebOutcome::Unaskable(format!("{why:#}")));
    }
    let gesture = match web::kill_switch_from_web(workspace, Some(check.agent)) {
        Ok(gesture) => gesture,
        Err(why) => return Ok(WebOutcome::Failed(format!("{why:#}"))),
    };
    let read = api_answer(&command_read(workspace, &gesture.command_id))?;
    Ok(WebOutcome::Recorded {
        agent: gesture.agent_id,
        workspace: gesture.workspace,
        command: gesture.command_id,
        read,
    })
}

/// The signed-in read of `command` in `workspace`: the API finds it on the workspace's control
/// stream and reads it accepted. Its `KillSwitchActivated` step appears only once a runtime
/// applies it, which waits on D1 and X, so the read does not require one.
pub fn command_read(workspace: &str, command: &str) -> ApiCheck {
    ApiCheck {
        shows: "the web gesture's command, read back",
        path: format!("/v1/workspaces/{workspace}/commands/{command}"),
        status: 200,
        ask: Ask::Session,
        body: Body::Command {
            id: command.to_owned(),
            stream: format!("ctl:{workspace}"),
        },
    }
}

/// What a run requires: the cell ([`CELL_REQUIRED_ENV`]) and the web gesture
/// ([`WEB_REQUIRED_ENV`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Required {
    pub cell: bool,
    pub web: bool,
}

/// How a web check reads: `ran` only when the gesture acted on its own agent in `workspace` and
/// the command reads back from the control stream, accepted; `failed` for a gesture that
/// failed, acted elsewhere, or reads back wrong, for an API that cannot be asked where the cell
/// is required, and for a machine without `web/`'s tools where the web gesture is; otherwise
/// `not run`. Returns the word, what to print after it, and whether the run fails.
pub fn web_verdict(
    outcome: &WebOutcome,
    check: &WebCheck,
    workspace: &str,
    required: Required,
) -> (&'static str, String, bool) {
    match outcome {
        WebOutcome::AfterFailure => ("not run", "an earlier part failed".into(), false),
        WebOutcome::Unaskable(why) if required.cell => ("failed", why.clone(), true),
        WebOutcome::NoTools(why) if required.web => ("failed", why.clone(), true),
        WebOutcome::Unaskable(why) | WebOutcome::NoTools(why) => ("not run", why.clone(), false),
        WebOutcome::Failed(why) => ("failed", why.clone(), true),
        WebOutcome::Recorded {
            agent,
            workspace: acted,
            command,
            read,
        } => {
            if agent != check.agent || acted != workspace {
                let said = format!(
                    "acted on {agent} in {acted}, not {} in {workspace}",
                    check.agent
                );
                return ("failed", said, true);
            }
            let (word, said, fails) = api_verdict(read, &command_read(workspace, command), true);
            let said = format!("command {command}: {said}");
            (if fails { word } else { "ran" }, said, fails)
        }
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
    use super::Readiness::{Partial, Ready, Skipped};
    use super::*;

    fn mandate() -> Vec<u8> {
        fs::read(format!("../{MANDATE_FILE}")).unwrap()
    }

    fn web_mandate() -> Vec<u8> {
        fs::read(format!("../{WEB_MANDATE_FILE}")).unwrap()
    }

    fn research_mandate() -> Vec<u8> {
        fs::read(format!("../{RESEARCH_MANDATE_FILE}")).unwrap()
    }

    /// The plan for `workspace` on the demo's three envelopes.
    fn planned(workspace: &str) -> Vec<Step> {
        plan(workspace, (&mandate(), &web_mandate(), &research_mandate()))
    }

    fn parsed(path: &str) -> Value {
        serde_json::from_slice(&fs::read(format!("../{path}")).unwrap()).unwrap()
    }

    /// A version is the SHA-256 of the file only when the file is its own canonical form: keys
    /// sorted, no whitespace. serde_json's map is ordered, and the files hold no number but small
    /// integers and no string but ASCII, where its compact form is RFC 8785's.
    #[test]
    fn the_demo_files_are_canonical_json() {
        for path in [
            MANDATE_FILE,
            INSTRUMENT_FILE,
            WEB_MANDATE_FILE,
            WEB_INSTRUMENT_FILE,
            RESEARCH_MANDATE_FILE,
        ] {
            let bytes = fs::read(format!("../{path}")).unwrap();
            let again = serde_json::to_vec(&parsed(path)).unwrap();
            assert_eq!(bytes, again, "{path} is not canonical JSON");
            assert!(bytes.is_ascii(), "{path} holds a non-ASCII byte");
        }
    }

    /// Every gesture's `--store` is the workspace's shared artifact store at the chart's own path,
    /// which the control runs write and the API server reads, so the API serves what the gestures
    /// stored, the backtest's report among them; a pod's own `/work` is a volume no other pod
    /// sees.
    #[test]
    fn the_gestures_write_the_store_the_api_reads() {
        let helpers =
            fs::read_to_string("../deploy/cell/charts/workspace/templates/_helpers.tpl").unwrap();
        let path = helpers
            .lines()
            .find(|line| line.contains(r#"define "mandate.artifactsPath""#))
            .and_then(|line| line.split("}}").nth(1))
            .and_then(|rest| rest.split("{{").next())
            .map(str::trim);
        assert_eq!(
            path,
            Some(STORE),
            "{STORE} is not the chart's artifact store path"
        );
        assert!(
            helpers.contains(r#"$access := dict "api" "seed" "control" "rw""#),
            "the control runs write the store and the API server reads it"
        );
        assert!(
            !STORE.starts_with("/work"),
            "{STORE} is the pod's own volume"
        );
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

    /// The second agent's envelope is the demo's but for its name, its description and its one
    /// pinned instrument, which its own snapshot names and which is not the demo agent's, so V-006
    /// finds it unclaimed on the shared connection (DEC-2036).
    #[test]
    fn the_web_envelope_is_the_demo_s_on_another_instrument() {
        let (mut demo, mut web) = (parsed(MANDATE_FILE), parsed(WEB_MANDATE_FILE));
        let instrument = parsed(WEB_INSTRUMENT_FILE);
        let pinned = web["universe"]["pinned_instruments"].clone();
        assert_eq!(pinned.as_array().map(Vec::len), Some(1));
        assert_eq!(pinned[0]["asset_id"], instrument["instrument_id"]);
        assert_eq!(pinned[0]["symbol"], instrument["symbol"]);
        assert_ne!(
            pinned[0]["asset_id"],
            parsed(INSTRUMENT_FILE)["instrument_id"]
        );
        assert_eq!(web["name"], WEB_AGENT);
        assert_eq!(web["connection_id"], demo["connection_id"]);
        for m in [&mut demo, &mut web] {
            m["name"] = Value::Null;
            m["behavior"]["description"] = Value::Null;
            m["universe"]["pinned_instruments"] = Value::Null;
        }
        assert_eq!(web, demo, "every other member is the demo's");
        let mut snapshot = instrument.clone();
        let mut spy = parsed(INSTRUMENT_FILE);
        for s in [&mut snapshot, &mut spy] {
            for key in ["instrument_id", "symbol", "exchange"] {
                s[key] = Value::Null;
            }
        }
        assert_eq!(snapshot, spy, "a plain, whole-share listed fund, as SPY is");
    }

    /// DEC-530 item 4's codes for the demo's mandate and the research agent's, as the CLI binary
    /// showed them when the scenario's gestures were run against a local Postgres (and the CLI
    /// accepted them typed).
    #[test]
    fn the_codes_are_the_first_eight_hex_digits_of_the_gestures_hash() {
        let version = "sha256:19dd2428a410b41fa00ddc1b3d8d15d5775096ed223a20edf123c50adae292e6";
        assert_eq!(confirm_code(version), "e869cb19");
        assert_eq!(deploy_code(AGENT, version), "63fbe39f");
        assert_eq!(code(""), "e3b0c442");
        let research = reference(&research_mandate());
        assert_eq!(
            research,
            "sha256:c6c2018e6f60482ac6ae838f2535e92036f21c6638f9818df5028d45537cd9de"
        );
        assert_eq!(confirm_code(&research), "9d06451c");
        assert_eq!(deploy_code(RESEARCH_AGENT, &research), "a45a1728");
    }

    /// The steps run 1 to 6 in order; a shown code precedes the gesture that types it, and the
    /// typed code is the one shown; every gesture's container name is unique and a Kubernetes name.
    #[test]
    fn the_plan_runs_the_six_steps_and_types_the_codes_it_shows() {
        let steps = planned("demo1");
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

    /// The payload the scenario records is the one the runner's own test commits: of the demo's
    /// version, under [`BACKTEST_ID`], completed, naming [`DEMO_REPORT`].
    #[test]
    fn the_demo_payload_is_the_runners_committed_run() {
        let golden: Value = serde_json::from_str(
            &fs::read_to_string("../crates/mandate-backtest-runner/tests/fixtures/demo-store.json")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(golden["payload"], DEMO_PAYLOAD);
        let payload = golden["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["bytes"].as_str().unwrap())
            .find(|bytes| reference(bytes.as_bytes()) == DEMO_PAYLOAD)
            .expect("the store holds its payload");
        let payload: Value = serde_json::from_str(payload).unwrap();
        assert_eq!(payload["report"], DEMO_REPORT);
        assert_eq!(payload["result"], "completed");
        assert_eq!(payload["backtest_id"], BACKTEST_ID);
        assert_eq!(payload["mandate_version"], reference(&mandate()));
    }

    /// Step 5 registers a rule set, runs the runner on the confirmed version in the pod's store,
    /// and records the payload it prints, after the confirmation and before the kill switch; the
    /// API read and web/'s report wait on the membership parser and web/.
    #[test]
    fn step_five_registers_a_rule_set_runs_the_backtest_and_records_it() {
        let steps = planned("demo5");
        let all = gestures(&steps);
        let at = |name: &str| {
            all.iter()
                .rposition(|g| g.container.ends_with(name))
                .unwrap()
        };
        let version = reference(&mandate());
        let (rule_set, runner, record) = (
            all.iter()
                .position(|g| g.args.iter().any(|a| a == "rule_set"))
                .unwrap(),
            at("runner-run"),
            at("backtest-record"),
        );
        let (confirm, kill) = (
            all.iter()
                .rposition(|g| g.args[..3] == ["version", "confirm", version.as_str()])
                .unwrap(),
            at("agent-kill"),
        );
        assert!(confirm < rule_set && rule_set < runner && runner < record && record < kill);
        assert!(
            all[rule_set]
                .args
                .windows(2)
                .any(|w| w == ["--kind", "rule_set"])
        );
        let run = all[runner];
        assert_eq!(run.command, Some(RUNNER_BIN));
        let version = reference(&mandate());
        for pair in [
            ["--inputs", BACKTEST_IN],
            ["--store", STORE],
            ["--mandate-version", version.as_str()],
            ["--backtest-id", BACKTEST_ID],
        ] {
            assert!(
                run.args.windows(2).any(|w| w == pair),
                "{pair:?} in {:?}",
                run.args
            );
        }
        assert_eq!(run.expect, format!("payload={DEMO_PAYLOAD}"));
        assert!(
            all[record]
                .args
                .windows(2)
                .any(|w| w == ["--payload", DEMO_PAYLOAD])
        );
        assert!(
            !all[record].args.iter().any(|a| a == "--user"),
            "the runner system records it"
        );
        assert_eq!(all[record].expect, format!("report={DEMO_REPORT}"));
        assert!(all.iter().filter(|g| g.command.is_some()).count() == 1);
        let lanes: Vec<&str> = steps[4]
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Gap(g) => Some(g.waits.iter().map(|w| w.lane)),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(lanes, ["W2"], "only web/'s report screen is left");
    }

    /// The runner's five inputs are read from its committed fixtures into their own directory of
    /// the pod's inputs, the one step 5 passes to `--inputs`.
    #[test]
    fn the_runners_inputs_go_to_their_own_directory() {
        let files = backtest_inputs(Path::new("..")).unwrap();
        let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                "backtest/bars.json",
                "backtest/outputs.json",
                "backtest/theses.json",
                "backtest/instrument.json",
                "backtest/settings.json"
            ]
        );
        assert_eq!(format!("{INPUTS}/backtest"), BACKTEST_IN);
        let settings: Value = serde_json::from_str(&files[4].1).unwrap();
        assert!(
            settings["gate"].is_object(),
            "the rule set is the run's settings"
        );
    }

    /// Every gesture states the workspace it acts on, or its control stream, never the live
    /// environment, and names the pod's own journal and store; the backtest runner, which holds no
    /// journal, names only that store. The record is exported last and verified against that
    /// store, so a record's missing artifact fails the run.
    #[test]
    fn every_gesture_is_in_the_demo_workspace_and_never_live() {
        let steps = planned("demo7");
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
            let runner = g.command == Some(RUNNER_BIN);
            let scoped = matches!(value(&g.args, "--workspace"), Some("demo7" | "demo7r"))
                || (export && g.args[2] == "ctl:demo7")
                || verify
                || runner;
            assert!(scoped, "{:?}", g.args);
            assert!(!g.args.iter().any(|a| a == "live"), "{:?}", g.args);
            assert_eq!(
                runner,
                value(&g.args, "--journal").is_none() && !verify,
                "{:?}",
                g.args
            );
            if !verify && !runner {
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

    /// The research agent's envelope: paper, every admission and opening asked, an unpinned
    /// universe of one instrument, a research envelope, and exactly one `llm.` model admitting
    /// instruments beside the demo's quant model, sorted by id (V-009, V-036; DEC-2246 item 4). Its
    /// connection is the demo's.
    #[test]
    fn the_research_envelope_researches_one_instrument_and_asks_everything() {
        let m = parsed(RESEARCH_MANDATE_FILE);
        assert_eq!(m["environment"], "paper");
        assert_eq!(m["autonomy"]["default"], "ask");
        assert_eq!(m["autonomy"]["admission"], "ask");
        assert_eq!(m["autonomy"]["rules"], json!([]));
        assert_eq!(m["universe"]["pinned"], false);
        assert_eq!(m["universe"]["pinned_instruments"], json!([]));
        assert_eq!(m["universe"]["max_instruments"], 1);
        assert_eq!(
            m["behavior"]["research"],
            json!({"cost_cap_usd_per_day": "5", "interval_s": 3600, "max_revisions_per_lineage": 3})
        );
        let models: Vec<(&str, bool)> = m["behavior"]["signal_models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| {
                (
                    model["id"].as_str().unwrap(),
                    model["admits_instruments"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            models,
            [("llm.research_agent", true), ("quant.ma_crossover", false)]
        );
        assert_eq!(m["connection_id"], parsed(MANDATE_FILE)["connection_id"]);
    }

    /// Step 3 opens and founds the research agent's own workspace, registers its two models
    /// there, the research one from the recorded entry, then creates, confirms and deploys its
    /// envelope, every gesture in that workspace and none in the run's, so the research agent is
    /// alone on its control stream (DEC-2214 item 1, DEC-2247 item 2). What is left waits on the
    /// manager starting a runtime from the deployment only (DEC-2247 item 3).
    #[test]
    fn step_three_deploys_the_research_agent_alone_in_its_own_workspace() {
        let steps = planned("demo3");
        let step = &steps[2];
        assert_eq!(step.number, 3);
        let gestures: Vec<&Gesture> = step
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Gesture(g) => Some(g),
                _ => None,
            })
            .collect();
        let words: Vec<[&str; 2]> = gestures
            .iter()
            .map(|g| [g.args[0].as_str(), g.args[1].as_str()])
            .collect();
        assert_eq!(
            words,
            [
                ["workspace", "open"],
                ["workspace", "found"],
                ["model", "register"],
                ["model", "register"],
                ["version", "create"],
                ["version", "confirm"],
                ["version", "confirm"],
                ["agent", "deploy"],
                ["agent", "deploy"],
            ]
        );
        for g in &gestures {
            let at = g.args.iter().position(|a| a == "--workspace").unwrap();
            assert_eq!(g.args[at + 1], "demo3r", "{:?}", g.args);
        }
        assert_eq!(
            gestures[3].args[2..6],
            [
                "llm.research_agent",
                "0.1.0",
                "--entry",
                "/in/research.json"
            ]
        );
        let version = reference(&research_mandate());
        assert_eq!(gestures[4].expect, format!("created {version}"));
        let deployed = &gestures[8].args;
        assert_eq!(deployed[2..4], [RESEARCH_AGENT, version.as_str()]);
        assert_eq!(lanes(step), ["X"], "only the manager starting its runtime");
        let others: Vec<&Gesture> = gestures_of(&steps, |n| n != 3);
        assert!(
            others.iter().all(|g| !g.args.iter().any(|a| a == "demo3r")),
            "no other step touches the research workspace"
        );
        assert!(
            others
                .iter()
                .all(|g| !g.args.iter().any(|a| a == RESEARCH_AGENT)),
            "the research agent is deployed nowhere else"
        );
    }

    /// The gestures of the steps whose number `keep` admits.
    fn gestures_of(steps: &[Step], keep: impl Fn(u8) -> bool) -> Vec<&Gesture> {
        steps
            .iter()
            .filter(|s| keep(s.number))
            .flat_map(|s| &s.parts)
            .filter_map(|p| match p {
                Part::Gesture(g) => Some(g),
                _ => None,
            })
            .collect()
    }

    /// Step 6 commits the owner's kill switch for the demo's agent, at agent scope only (an
    /// agent-scoped switch never reaches the connection or the workspace, rule 13), after the
    /// agent is deployed and before the record is exported, so the closing verify covers it.
    #[test]
    fn step_six_kills_the_demo_agent_at_agent_scope_before_the_record() {
        let steps = planned("demo1");
        let all = gestures(&steps);
        let at = |head: &[&str]| all.iter().position(|g| g.args[..head.len()] == *head);
        let kill = at(&["agent", "kill"]).expect("step 6 runs `agent kill`");
        let g = all[kill];
        assert_eq!(g.args[2..4], ["--agent", AGENT]);
        assert!(
            !g.args
                .iter()
                .any(|a| a == "--connection" || a == "--workspace-wide"),
            "{:?}",
            g.args
        );
        assert_eq!(
            g.expect,
            format!("kill_switch agent {AGENT} committed as event")
        );
        let deployed = all
            .iter()
            .rposition(|g| g.args[..2] == ["agent", "deploy"])
            .unwrap();
        assert!(deployed < kill, "the agent is deployed before it is killed");
        assert!(
            kill < at(&["journal", "export"]).unwrap(),
            "exported after it"
        );
        let step6 = &steps[5].parts;
        assert!(
            step6
                .iter()
                .any(|p| matches!(p, Part::Gesture(x) if x.container == g.container)),
            "the kill switch is step 6's"
        );
    }

    /// The API checks, in order: step 1's read of the run's own workspace with no session (the one
    /// 401), the demo owner's sign-in (204), and the read with that session, which must list the
    /// demo's agent; then step 5's signed-in read of the backtest, completed, with the stored
    /// report of the confirmed version.
    #[test]
    fn the_api_checks_refuse_sign_in_and_read_the_run_as_its_owner() {
        let ws = workspace_of("0123456789");
        let steps = planned(&ws);
        let checks: Vec<(u8, &str, u16, Ask, &Body)> = steps
            .iter()
            .flat_map(|s| s.parts.iter().map(move |p| (s.number, p)))
            .filter_map(|(n, p)| match p {
                Part::Api(c) => Some((n, c.path.as_str(), c.status, c.ask, &c.body)),
                _ => None,
            })
            .collect();
        let read = format!("/v1/workspaces/{ws}/agents");
        let backtest = format!("/v1/workspaces/{ws}/backtests/{BACKTEST_ID}");
        let completed = Body::Members(vec![
            ("/backtest/state", "completed".into()),
            ("/backtest/report_ref", DEMO_REPORT.into()),
            ("/backtest/mandate_version", reference(&mandate())),
        ]);
        assert_eq!(
            checks,
            [
                (1, read.as_str(), 401, Ask::NoSession, &Body::Any),
                (1, "/v1/sessions", 204, Ask::SignIn, &Body::Any),
                (1, read.as_str(), 200, Ask::Session, &Body::Lists(AGENT)),
                (5, backtest.as_str(), 200, Ask::Session, &completed),
            ]
        );
        assert!(
            !by_lane(&steps).iter().any(|(lane, _)| *lane == "MB"),
            "nothing waits on the membership parser now on main"
        );
    }

    /// A body check holds only for what it names: the agent among `agents`, and each member at
    /// its pointer as that exact string.
    #[test]
    fn a_body_check_holds_only_for_what_it_names() {
        let lists = Body::Lists("demo-spy");
        assert_eq!(
            lists.missing(r#"{"agents":[{"agent_id":"x"},{"agent_id":"demo-spy"}]}"#),
            None
        );
        for body in [
            r#"{"agents":[{"agent_id":"demo-web"}]}"#,
            r#"{"agents":[]}"#,
            "{}",
            "<html>",
        ] {
            assert!(lists.missing(body).is_some(), "{body}");
        }
        let members = Body::Members(vec![("/b/state", "completed".into()), ("/b/r", "x".into())]);
        assert_eq!(
            members.missing(r#"{"b":{"state":"completed","r":"x"}}"#),
            None
        );
        for body in [
            r#"{"b":{"state":"running","r":"x"}}"#,
            r#"{"b":{"state":"completed","r":"y"}}"#,
            r#"{"b":{"state":"completed"}}"#,
            r#"{"b":{"state":"completed","r":1}}"#,
        ] {
            assert!(members.missing(body).is_some(), "{body}");
        }
        assert_eq!(Body::Any.missing("<html>"), None);
        let session = check(200, Ask::Session);
        let wanting = ApiCheck {
            body: lists,
            ..session
        };
        let reply = |body: &str| Ok((200, format!("HTTP/1.1 200 OK\r\n\r\n{body}")));
        assert_eq!(
            api_verdict(
                &reply(r#"{"agents":[{"agent_id":"demo-spy"}]}"#),
                &wanting,
                false
            )
            .0,
            "ran"
        );
        let (word, said, fails) = api_verdict(&reply(r#"{"agents":[]}"#), &wanting, false);
        assert_eq!((word, fails), ("failed", true), "{said}");
        let refused = "HTTP/1.1 401 Unauthorized\r\n\r\n".to_owned();
        for answer in [
            Ok((401, refused)),
            Ok((204, "HTTP/1.1 204 No Content\r\n\r\n".into())),
        ] {
            let (word, said, fails) = api_verdict(&answer, &wanting, false);
            assert_eq!(
                (word, fails),
                ("failed", true),
                "a sign-in with no cookie is a wrong answer, never not run: {said}"
            );
        }
    }

    /// The second agent in the run's workspace has an envelope of its own, deployed only after
    /// step 5's backtest is recorded (the research agent deploys in its own workspace, step 3): `agent deploy` and `backtest record` each take only the workspace's latest
    /// confirmed version, so its confirmation comes after the record and its deployment after its
    /// confirmation. Its snapshot is registered before its version is created, and web/ presses
    /// that agent's kill switch, never the one the CLI stops (DEC-2036).
    #[test]
    fn web_kills_a_second_agent_the_cli_does_not() {
        let (version, web) = (reference(&mandate()), reference(&web_mandate()));
        assert_ne!(version, web, "two envelopes, two versions");
        let steps = planned("demo6");
        let all = gestures(&steps);
        let deployed: Vec<[&str; 2]> = all
            .iter()
            .filter(|g| g.args[..2] == ["agent", "deploy"] && g.args.iter().any(|a| a == "demo6"))
            .map(|g| [g.args[2].as_str(), g.args[3].as_str()])
            .collect();
        let (v, w) = (version.as_str(), web.as_str());
        assert_eq!(
            deployed,
            [[AGENT, v], [AGENT, v], [WEB_AGENT, w], [WEB_AGENT, w]]
        );
        let code = deploy_code(WEB_AGENT, &web);
        assert_ne!(code, deploy_code(AGENT, &version));
        assert!(
            all.iter().any(|g| g.args[2] == WEB_AGENT
                && g.args.windows(2).any(|w| w == ["--code", code.as_str()]))
        );
        let at = |args: &[&str]| {
            all.iter()
                .position(|g| g.args.len() >= args.len() && g.args[..args.len()] == *args)
                .unwrap_or_else(|| panic!("no gesture {args:?}"))
        };
        let snapshot = format!("{INPUTS}/web-instrument.json");
        let register = at(&[
            "config",
            "register",
            "--kind",
            "instrument_snapshot",
            &snapshot,
        ]);
        let created = at(&["version", "create", &format!("{INPUTS}/web-mandate.json")]);
        let confirmed = at(&["version", "confirm", w, "--code"]);
        let deploy = at(&["agent", "deploy", WEB_AGENT, w, "--code"]);
        let record = at(&["backtest", "record"]);
        assert!(at(&["agent", "deploy", AGENT, v, "--code"]) < record);
        assert!(record < register && register < created && created < confirmed);
        assert!(confirmed < deploy);
        assert_eq!(all[created].expect, format!("created {web}"));
        let step_of = |container: &str| {
            steps
                .iter()
                .find(|s| {
                    s.parts
                        .iter()
                        .any(|p| matches!(p, Part::Gesture(g) if g.container == container))
                })
                .map(|s| s.number)
        };
        assert_eq!(step_of(&all[register].container), Some(6));
        assert_eq!(step_of(&all[deploy].container), Some(6));
        let kill = all
            .iter()
            .find(|g| g.args[..2] == ["agent", "kill"])
            .unwrap();
        assert!(
            kill.args.windows(2).any(|w| w == ["--agent", AGENT]),
            "the CLI stops demo-spy"
        );
        let web: Vec<(u8, &WebCheck)> = steps
            .iter()
            .flat_map(|s| s.parts.iter().map(move |p| (s.number, p)))
            .filter_map(|(n, p)| match p {
                Part::Web(w) => Some((n, w)),
                _ => None,
            })
            .collect();
        assert_eq!(web.len(), 1);
        assert_eq!(web[0].0, 6, "web/'s kill switch is step 6's");
        assert_eq!(web[0].1.agent, WEB_AGENT);
        assert_ne!(WEB_AGENT, AGENT);
    }

    /// A web check runs only when the gesture acted on its own agent in the run's workspace and
    /// the command reads back with its kill switch on the control stream; a machine without
    /// web/'s tools never fails the run, a failed gesture always does.
    #[test]
    fn a_web_check_runs_only_on_its_own_recorded_kill_switch() {
        let check = WebCheck {
            shows: "s",
            agent: WEB_AGENT,
        };
        let ws = "01KDEM0000000001000000000D";
        let body = |command: &str, stream: &str, phase: &str| {
            let read = json!({"command_id": command, "phase": phase, "steps": [],
                "as_of": [{"stream_id": stream, "seq": 10}]});
            format!("HTTP/1.1 200 OK\r\n\r\n{read}")
        };
        let recorded =
            |agent: &str, acted: &str, read: Result<(u16, String), String>| WebOutcome::Recorded {
                agent: agent.into(),
                workspace: acted.into(),
                command: "C1".into(),
                read,
            };
        let ctl = format!("ctl:{ws}");
        for cell in [false, true] {
            let required = Required { cell, web: false };
            for phase in COMMAND_PHASES {
                let ran = recorded(WEB_AGENT, ws, Ok((200, body("C1", &ctl, phase))));
                assert_eq!(web_verdict(&ran, &check, ws, required).0, "ran", "{phase}");
            }
            for wrong in [
                recorded(AGENT, ws, Ok((200, body("C1", &ctl, "recorded")))),
                recorded(WEB_AGENT, "other", Ok((200, body("C1", &ctl, "recorded")))),
                recorded(
                    WEB_AGENT,
                    ws,
                    Ok((200, body("C1", "ctl:other", "recorded"))),
                ),
                recorded(WEB_AGENT, ws, Ok((200, body("C1", &ctl, "refused")))),
                recorded(WEB_AGENT, ws, Ok((200, body("C2", &ctl, "recorded")))),
                recorded(WEB_AGENT, ws, Ok((404, body("C1", &ctl, "recorded")))),
                recorded(WEB_AGENT, ws, Err("no answer".into())),
                WebOutcome::Failed("the run failed".into()),
            ] {
                let (word, said, fails) = web_verdict(&wrong, &check, ws, required);
                assert_eq!((word, fails), ("failed", true), "{said}");
            }
            assert_eq!(
                web_verdict(&WebOutcome::NoTools("no npm".into()), &check, ws, required),
                ("not run", "no npm".into(), false),
                "the cell does not require web/'s tools"
            );
        }
        let unaskable = WebOutcome::Unaskable("no kubectl".into());
        let only = |cell, web| Required { cell, web };
        assert!(!web_verdict(&unaskable, &check, ws, only(false, false)).2);
        assert!(web_verdict(&unaskable, &check, ws, only(true, false)).2);
        assert!(
            !web_verdict(&unaskable, &check, ws, only(false, true)).2,
            "the web switch alone does not require the cell"
        );
        let no_tools = WebOutcome::NoTools("no npm".into());
        assert_eq!(
            web_verdict(&no_tools, &check, ws, only(false, false)),
            ("not run", "no npm".into(), false)
        );
        for cell in [false, true] {
            assert_eq!(
                web_verdict(&no_tools, &check, ws, only(cell, true)),
                ("failed", "no npm".into(), true),
                "where the web gesture is required, a machine without its tools fails"
            );
        }
        for required in [only(false, false), only(true, true)] {
            assert_eq!(
                web_verdict(&WebOutcome::AfterFailure, &check, ws, required),
                ("not run", "an earlier part failed".into(), false),
                "the first failure is the run's; this one only reads not run"
            );
        }
        let read = command_read(ws, "C1");
        assert_eq!(read.path, format!("/v1/workspaces/{ws}/commands/C1"));
        assert_eq!(read.ask, Ask::Session);
    }

    /// The session cookie's pair is read from the reply's headers only, by its own name.
    #[test]
    fn the_session_cookie_is_read_from_the_headers() {
        let reply = "HTTP/1.1 204 No Content\r\nSet-Cookie: other=1\r\nset-cookie: \
                     __Host-mandate_session=v1.abc; Path=/; Secure\r\n\r\n";
        assert_eq!(
            session_cookie(reply).as_deref(),
            Some("__Host-mandate_session=v1.abc")
        );
        assert_eq!(
            session_cookie("HTTP/1.1 204 No Content\r\n\r\nSet-Cookie: __Host-mandate_session=x"),
            None
        );
        assert_eq!(
            session_cookie("HTTP/1.1 204 No Content\r\nSet-Cookie: other=1\r\n\r\n"),
            None
        );
    }

    /// A run's workspace is a ULID (26 Crockford base-32 characters, the first at most 7), fresh
    /// for each run id, and never the fixed workspace the chart's values name.
    #[test]
    fn each_run_takes_a_fresh_ulid_workspace() {
        let crockford =
            |c: char| c.is_ascii_digit() || (c.is_ascii_uppercase() && !"ILOU".contains(c));
        let a = workspace_of("0123456789");
        let b = workspace_of("9876543210");
        for ws in [&a, &b, &workspace_of("7")] {
            assert_eq!(ws.len(), 26, "{ws}");
            assert!(ws.chars().all(crockford), "{ws}");
            assert!(ws.as_bytes()[0] <= b'7', "{ws}");
            assert_ne!(ws.as_str(), super::super::DEMO_WORKSPACE);
        }
        assert_ne!(a, b);
        assert_eq!(a, "01KDEM0123456789000000000D");
        let steps = planned(&a);
        let all = gestures(&steps);
        let owner = super::super::DEMO_OWNER;
        let found = all
            .iter()
            .find(|g| g.args[..2] == ["workspace", "found"])
            .unwrap();
        assert!(
            found.args.windows(2).any(|w| w == ["--member", owner]),
            "the workspace is founded for the owner who signs in: {:?}",
            found.args
        );
        let kill = all
            .iter()
            .find(|g| g.args[..2] == ["agent", "kill"])
            .unwrap();
        let user = owner.to_ascii_lowercase();
        assert!(
            kill.args
                .windows(2)
                .any(|w| w[0] == "--user" && w[1] == user),
            "the owner's gestures act as that member: {:?}",
            kill.args
        );
    }

    fn check(status: u16, ask: Ask) -> ApiCheck {
        ApiCheck {
            shows: "s",
            path: "/".into(),
            status,
            ask,
            body: Body::Any,
        }
    }

    /// Only the expected status reads as run, and a sign-in only with the session cookie; any
    /// other answer always fails the run, and an API that cannot be asked fails it only where CI
    /// requires the cell.
    #[test]
    fn an_api_check_runs_only_on_its_answer_and_fails_where_the_cell_is_required() {
        let refusal = check(401, Ask::NoSession);
        let sign_in = check(204, Ask::SignIn);
        let opened = "HTTP/1.1 204 No Content\r\nset-cookie: __Host-mandate_session=v1.a; \
                      Path=/\r\n\r\n"
            .to_owned();
        let bare = "HTTP/1.1 204 No Content\r\n\r\n".to_owned();
        let unreachable: Result<(u16, String), String> = Err("no kubectl".into());
        for required in [false, true] {
            assert_eq!(
                api_verdict(&Ok((401, String::new())), &refusal, required),
                ("ran", "answered 401".into(), false),
                "a 401 never fails the refusal check"
            );
            assert_eq!(
                api_verdict(&Ok((204, opened.clone())), &sign_in, required).0,
                "ran"
            );
            for (status, reply) in [
                (204, bare.clone()),
                (401, opened.clone()),
                (403, bare.clone()),
            ] {
                let (word, _, fails) = api_verdict(&Ok((status, reply)), &sign_in, required);
                assert_eq!((word, fails), ("failed", true), "{status}");
            }
            for status in [200, 403, 404, 500] {
                let (word, _, fails) =
                    api_verdict(&Ok((status, String::new())), &refusal, required);
                assert_eq!(
                    (word, fails),
                    ("failed", true),
                    "{status}, required {required}"
                );
            }
        }
        assert_eq!(
            api_verdict(&unreachable, &sign_in, false),
            ("not run", "no kubectl".into(), false)
        );
        assert_eq!(
            api_verdict(&unreachable, &sign_in, true),
            ("failed", "no kubectl".into(), true)
        );
    }

    /// A wrong status from a reachable API is an answer, even when the port-forward bailed on it,
    /// so the verdict fails it; only a probe that reads nothing makes the API unreachable.
    #[test]
    fn a_reachable_api_that_answers_wrongly_is_an_answer_not_unreachable() {
        let refusal = check(401, Ask::NoSession);
        let bailed = || Err(anyhow::anyhow!("answered 404, not the one 401"));
        let wrong = || Ok((404, String::new()));
        assert_eq!(answer_of(bailed(), wrong), Ok((404, String::new())));
        assert_eq!(
            api_verdict(&answer_of(bailed(), wrong), &refusal, false).0,
            "failed"
        );
        let refused = || Err::<(u16, String), _>(anyhow::anyhow!("connection refused"));
        assert!(answer_of(bailed(), refused).is_err(), "nothing answered");
        assert!(answer_of(Ok(()), refused).is_err(), "nothing answered");
        assert_eq!(
            api_verdict(&answer_of(Ok(()), refused), &refusal, false).0,
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
        let steps = planned("demo1");
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

    /// Which steps run today: the gestures on `main` make steps 1, 2, 4, 5 and 6 partial, step 3
    /// waits wholly on other lanes, and the record runs whole.
    #[test]
    fn each_step_reads_ready_partial_or_skipped_from_its_parts() {
        let steps = planned("demo1");
        let read: Vec<Readiness> = steps.iter().map(readiness).collect();
        assert_eq!(
            read,
            [Partial, Partial, Partial, Partial, Partial, Partial, Ready]
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
        let check = || Part::Api(check(401, Ask::NoSession));
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
        let steps = planned("demo1");
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
        let steps = planned("demo1");
        let all = gestures(&steps);
        let files = [
            ("mandate.json", "{}".to_string()),
            ("backtest/bars.json", "{}".to_string()),
        ];
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
            json!({"name": "in", "configMap": {"name": "demo-in-1", "items": [
                {"key": "mandate.json", "path": "mandate.json"},
                {"key": "backtest.bars.json", "path": "backtest/bars.json"}
            ]}})
        );
        assert_eq!(
            objects[1]["data"],
            json!({"mandate.json": "{}", "backtest.bars.json": "{}"})
        );
        let runner = spec["initContainers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"].as_str().unwrap().ends_with("runner-run"))
            .unwrap();
        assert_eq!(runner["command"], json!([RUNNER_BIN]));
        assert!(
            last.get("command").is_none(),
            "the CLI keeps the image's entry point"
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
