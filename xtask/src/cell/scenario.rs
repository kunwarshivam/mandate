//! `cargo xtask cell demo --scenario demo` (E21-54, DEC-2002): the demo's story in order, the
//! story's six steps with the owner's exit as step 6, before the kill switch, on the local cell,
//! with paper and demo data only and no credential. Every gesture whose pieces are on `main` runs for real, in one Job of the cell's own `control` process, so the
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
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::executor::{CONNECTION, connect_check, executor_live};
use super::{
    API_LOCAL_PORT, API_PROCESS, CELL_REQUIRED_ENV, DEMO_OWNER, DEPLOY_MANAGER_PROCESS,
    ImageSource, NAMESPACE, NODE, WORKSPACE_CHART, api_diagnosis, api_image_tag, api_port_forward,
    helm, host_image_source, http, kubectl, open_session, processes, render, render_with,
    reply_body, run_id, run_job, session_cookie, signed_in, stub_image_tag, stub_token,
    template_args, tools_missing, up, web,
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
/// The instrument the demo's envelope pins (SPY, [`INSTRUMENT_FILE`]'s `instrument_id`), which the
/// owner's exit names.
pub const SPY: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
/// The bid, bid size and floor the owner's exit confirms, as the CLI's canonical text (DEC-2507
/// item 1): fixed values the scenario types, since the cell shows no live quote.
const EXIT_BID: (&str, &str, &str) = ("501.25", "100", "500");
/// A second agent, on its own envelope ([`WEB_MANDATE_FILE`]), deployed for `web/`'s kill switch:
/// the CLI stops [`AGENT`] inside the Job, and `web/` offers nothing to stop on a stopped agent, so
/// the browser presses this one's (lane W2's D10). Its id is `agt_` and a ULID, the only agent id
/// web/'s routes open (`web/src/lib/ids.ts`'s `AGENT_ID`); any other id is a 404 there.
pub const WEB_AGENT: &str = "agt_01KDEM00000000000000000WEB";
/// [`WEB_AGENT`]'s envelope's `name`: a mandate name is lowercase letters, digits and hyphens
/// (`mandate.schema.json`), so it cannot be the agent's id, and nothing requires that it is.
#[cfg(test)]
const WEB_ENVELOPE: &str = "demo-web";
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

/// The run's account stream (DEC-2379): a ULID, fresh for each run id, so no earlier run's
/// connection binds it.
pub fn account_of(id: &str) -> String {
    format!("01KDEA{id:0>10.10}000000000A")
}

/// The demo's paper connection as the executor's connect check left it: the account the
/// connection binds and the passing `ConnectionChecked` the record names as its cause.
#[derive(Debug, Clone)]
pub struct Connected {
    pub account: String,
    pub checked: String,
}

/// The run's workspace (DEC-2010): a ULID, since the API hosts only a ULID, fresh for each run id
/// because a confirmed version refuses a second confirmation: `01KDEM`, the run id's ten digits,
/// then a fixed tail that no other demo id uses.
pub fn workspace_of(id: &str) -> String {
    format!("01KDEM{id:0>10.10}000000000D")
}

/// One of the demo's steps and what it is made of.
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
    WebSignIn(WebCheck),
    WebTradeLoop(WebCheck),
    Executor(ExecutorCheck),
    Applied(AppliedCheck),
    Gap(Gap),
}

/// The deployment manager's executor for `account`, started from the agent's version-2
/// deployment, running and answering `livez` after the gestures' Job (DEC-2035, DEC-2379).
#[derive(Debug)]
pub struct ExecutorCheck {
    pub shows: &'static str,
    pub account: String,
}

/// The owner command the gesture just before it in its step committed, read back with the
/// owner's session until its phase is `applied` with `agent`'s runtime's `KillSwitchActivated` on
/// the agent's stream (DEC-2379). That gesture's `committed as event <id>` names the command.
#[derive(Debug)]
pub struct AppliedCheck {
    pub shows: &'static str,
    pub agent: &'static str,
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
const DR_RESEARCH: Wait = Wait {
    lane: "DR",
    story: "E21-54",
    pr: None,
    lands: "the research workspace's own connect check and connection record, as step 2 runs \
            for the demo's workspace, so its deployment is written at version 2 and the manager \
            starts its runtime (DEC-1171)",
};
const DR_CONNECT: Wait = Wait {
    lane: "DR",
    story: "E21-54",
    pr: None,
    lands: "a connection the dry run can name: staging's comes from the owner's OAuth connect, \
            which the scenario does not run yet",
};
const P2_SCHEDULER: Wait = Wait {
    lane: "P2",
    story: "E21-29",
    pr: None,
    lands: "the scheduler in the cell, whose tick lets the executor start and reconcile, so it \
            is ready to take an exit; the cell builds no scheduler yet (`processes.json`)",
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
    lands: "slice X1b-3: paper fills on the fixture; the executor already takes the \
            runtime's orders (#1736)",
};
const P5_HOOK: Wait = Wait {
    lane: "D1",
    story: "E21-2",
    pr: None,
    lands: "the runtime's production ports, the post-R0 hook (DEC-968 item 6, DEC-1013), which \
            the founder approved under DEC-2378",
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
    Command {
        id: String,
        stream: String,
        phases: &'static [&'static str],
        step: Option<(&'static str, String)>,
    },
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
            Self::Command {
                id,
                stream,
                phases,
                step,
            } => {
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
                } else if !phases.contains(&phase) {
                    Some(format!("its phase is {phase:?}"))
                } else if let Some((event_type, on)) = step
                    && !value["steps"].as_array().is_some_and(|steps| {
                        steps
                            .iter()
                            .any(|s| s["event_type"] == *event_type && s["stream_id"] == *on)
                    })
                {
                    Some(format!("no {event_type} step on {on}"))
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

pub(super) fn confirm_code(version: &str) -> String {
    code(&format!(
        r#"{{"gesture":"mandate_confirm","mandate_version":"{version}"}}"#
    ))
}

/// The code `connection record` shows for the demo's paper connection on `account`, caused by the
/// connect check `checked`, for the owner `user` (DEC-530 item 4): its record's members, the
/// cause and the gesture, as canonical JSON.
pub fn record_code(account: &str, checked: &str, user: &str) -> String {
    code(&format!(
        concat!(
            r#"{{"account_ref":"{account}","broker":"alpaca","causation_id":"{checked}","#,
            r#""connection_id":"{connection}","environment":"paper","#,
            r#""gesture":"connection_record","margin_attestation":null,"#,
            r#""scopes":["trading"],"user":"{user}"}}"#
        ),
        account = account,
        checked = checked,
        connection = CONNECTION,
        user = user,
    ))
}

pub(super) fn deploy_code(agent: &str, version: &str) -> String {
    code(&format!(
        r#"{{"agent_id":"{agent}","gesture":"agent_deploy","mandate_version":"{version}"}}"#
    ))
}

/// The demo's steps for `workspace`, given the bytes of the demo's mandate, of
/// [`WEB_AGENT`]'s, and of [`RESEARCH_AGENT`]'s, and the paper connection the executor's connect
/// check left `connected`.
pub fn plan(
    workspace: &str,
    (mandate, web_mandate, research_mandate): (&[u8], &[u8], &[u8]),
    connected: Option<&Connected>,
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
    let record_args = |connected: &Connected, extra: &[&str]| {
        owner(
            &[
                &[
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
                    &connected.account,
                    "--checked",
                    &connected.checked,
                ][..],
                extra,
            ]
            .concat(),
        )
    };
    let (bid, bid_size, floor) = EXIT_BID;
    let exit_args = owner(&[
        "agent",
        "exit",
        AGENT,
        SPY,
        "--confirm-bid",
        bid,
        "--bid-size",
        bid_size,
        "--floor",
        floor,
    ]);
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
                Part::WebSignIn(WebCheck {
                    shows: "the demo owner, signed in to web/ through the stub issuer, reading \
                            the run's workspace's agents through the API",
                    agent: WEB_AGENT,
                }),
            ],
        },
        Step {
            number: 2,
            title: "the owner deploys an agent with a confirmed mandate envelope",
            parts: [
                match connected {
                    Some(connected) => {
                        let recorded = record_code(&connected.account, &connected.checked, &user);
                        vec![
                            gesture(
                                "the paper connection's record, its code shown (nothing \
                                 committed), naming the executor's passing connect check",
                                record_args(connected, &[]),
                                format!("code {recorded}"),
                            ),
                            gesture(
                                "the paper connection, recorded with the code shown",
                                record_args(connected, &["--code", &recorded]),
                                format!("recorded {CONNECTION} as event"),
                            ),
                        ]
                    }
                    None => vec![Part::Gap(Gap {
                        what: "the paper connection, recorded from its connect, so the \
                               deployment is written at version 2",
                        on_main: "the executor's connect check and its record in the local cell \
                                  (#1759, #1763), the API's OAuth connect start (#2018) and \
                                  callback (#2100)",
                        waits: vec![DR_CONNECT],
                    })],
                },
                vec![
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
                        on_main: "the API's version, confirm and deploy routes (#1552) in the \
                                  cell (#1554), web/'s set-up (#1557), its sign-in to a dev cell \
                                  (#1588) and the stub issuer (#1591)",
                        waits: vec![W2_WEB],
                    }),
                ],
                connected
                    .map(|connected| {
                        Part::Executor(ExecutorCheck {
                            shows: "the account's executor, started by the deployment manager \
                                    from the version-2 deployment, running and live",
                            account: connected.account.clone(),
                        })
                    })
                    .into_iter()
                    .collect(),
            ]
            .into_iter()
            .flatten()
            .collect(),
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
                              (#1779, #1782), and the connect check and record that write a \
                              deployment at version 2 (#1759, #1763), which step 2 runs",
                    waits: vec![DR_RESEARCH],
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
                              that proposes nothing (DEC-1013 item 2), started by the manager \
                              from step 2's version-2 deployment (#1759, #1763)",
                    waits: vec![P5_HOOK],
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
                Part::WebTradeLoop(WebCheck {
                    shows: "the trade loop from web/ on the second agent: its approval request \
                            shown, approved with the code the host confirms, the order filled, \
                            the position and the journal shown, then its kill switch",
                    agent: WEB_AGENT,
                }),
                Part::Gap(Gap {
                    what: "the paper fill and the protective stop on a fixture broker",
                    on_main: "the executor process (#1596, binary #1694) and its interim image \
                              in the cell on the fixture paper broker (#1702, DEC-2031), handing \
                              its agents' orders to the core (#1736), with the runtime proposing \
                              nothing on the deny-all wiring (DEC-1013)",
                    waits: vec![X_EXECUTOR, P5_HOOK],
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
            title: "the owner exits the agent's instrument at a confirmed floor",
            parts: vec![
                gesture(
                    "the owner's exit at the confirmed bid, size and floor, committed: a risk \
                     reduction, which needs no code (AGENTS.md rules 2 and 13)",
                    exit_args,
                    format!("owner_exit {AGENT} {SPY} committed as event"),
                ),
                Part::Gap(Gap {
                    what: "the exit handed to the executor's core, priced at the confirmed floor",
                    on_main: "the executor process hands its agents' orders to the core and \
                              prices an owner's exit at the floor it confirmed (#1736, DEC-2033 \
                              item 4), its image in the cell (#1702), started and live from step \
                              2's version-2 deployment (#1763)",
                    waits: vec![P2_SCHEDULER],
                }),
                Part::Gap(Gap {
                    what: "the exit's paper fill on the fixture broker, out of the position \
                           step 4 filled",
                    on_main: "the executor's interim image on the fixture paper broker (#1702, \
                              DEC-2031)",
                    waits: vec![X_EXECUTOR, P5_HOOK],
                }),
            ],
        },
        Step {
            number: 7,
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
                match connected {
                    Some(_) => Part::Applied(AppliedCheck {
                        shows: "the agent's runtime, started from step 2's version-2 \
                                deployment, applying the CLI's kill switch: its command read \
                                back applied with the owner's session",
                        agent: AGENT,
                    }),
                    None => Part::Gap(Gap {
                        what: "the agent's runtime applies it",
                        on_main: "the runtime reads the kill switch first (#1521, image #1560), \
                                  started by the manager from a version-2 deployment (#1763)",
                        waits: vec![DR_CONNECT],
                    }),
                },
            ],
        },
        Step {
            number: 8,
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
            Part::Api(_)
            | Part::Web(_)
            | Part::WebSignIn(_)
            | Part::WebTradeLoop(_)
            | Part::Executor(_)
            | Part::Applied(_)
            | Part::Gap(_) => None,
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
    namespace: &str,
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
        "metadata": {"name": configmap, "namespace": namespace, "labels": labels},
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

/// The demo's inputs: its three envelopes, and every file the gestures' pod mounts at [`INPUTS`]
/// under its path there (the envelopes, the instruments, the research recording and the backtest
/// runner's demo inputs).
pub struct DemoInputs {
    mandate: Vec<u8>,
    web_mandate: Vec<u8>,
    research_mandate: Vec<u8>,
    files: Vec<(String, String)>,
}

impl DemoInputs {
    /// The three envelopes [`plan`] takes.
    pub fn envelopes(&self) -> (&[u8], &[u8], &[u8]) {
        (&self.mandate, &self.web_mandate, &self.research_mandate)
    }

    /// The mounted files, as [`gestures_job`] takes them.
    pub fn files(&self) -> Vec<(&str, String)> {
        self.files
            .iter()
            .map(|(path, content)| (path.as_str(), content.clone()))
            .collect()
    }
}

/// Reads the demo's inputs from the checkout.
pub fn demo_inputs() -> Result<DemoInputs> {
    let read = |path: &str| fs::read(path).with_context(|| format!("reading {path}"));
    let text = |bytes: Vec<u8>, path: &str| {
        String::from_utf8(bytes).with_context(|| format!("{path} is not UTF-8"))
    };
    let (mandate, web_mandate, research_mandate) = (
        read(MANDATE_FILE)?,
        read(WEB_MANDATE_FILE)?,
        read(RESEARCH_MANDATE_FILE)?,
    );
    let files = [
        ("mandate.json", text(mandate.clone(), MANDATE_FILE)?),
        (
            "instrument.json",
            text(read(INSTRUMENT_FILE)?, INSTRUMENT_FILE)?,
        ),
        (
            "web-mandate.json",
            text(web_mandate.clone(), WEB_MANDATE_FILE)?,
        ),
        (
            "web-instrument.json",
            text(read(WEB_INSTRUMENT_FILE)?, WEB_INSTRUMENT_FILE)?,
        ),
        (
            "research-mandate.json",
            text(research_mandate.clone(), RESEARCH_MANDATE_FILE)?,
        ),
        (
            "research.json",
            text(read(RESEARCH_RECORDING_FILE)?, RESEARCH_RECORDING_FILE)?,
        ),
    ]
    .into_iter()
    .map(|(path, content)| (path.to_string(), content))
    .chain(backtest_inputs(Path::new("."))?)
    .collect();
    Ok(DemoInputs {
        mandate,
        web_mandate,
        research_mandate,
        files,
    })
}

/// The staging workspace's namespace, the one its overlay installs the workspace chart in
/// (`deploy/gitops/base/workspace.yaml`).
pub const STAGING_NAMESPACE: &str = "ws-ws1";

/// The values staging's HelmRelease renders the workspace chart with, in its order: the managed
/// cluster's, then staging's images and tier (`deploy/gitops/base/kustomization.yaml`,
/// `deploy/gitops/clusters/staging/images.yaml`).
pub const STAGING_VALUES: &[&str] = &[
    "deploy/cell/values/managed.yaml",
    "deploy/gitops/clusters/staging/images.yaml",
];

/// Where a dry run renders the demo's gestures for: a namespace, the released `mandate` image the
/// gestures run, pinned by digest (staging admits only signed release images, DEC-2016), and the
/// fixed workspace the cell's API hosts (the coordinator's option (a)). It names no kubeconfig:
/// the dry run reaches no cluster.
#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub namespace: String,
    pub image: String,
    pub workspace: String,
}

/// The target `--image <ref> --workspace <id> [--namespace <ns>]` names, [`STAGING_NAMESPACE`] by
/// default. Refuses an image not pinned by a SHA-256 digest, a workspace that is not an opaque
/// ULID (identity spec §3.1), an unknown or repeated flag, and a flag with no value.
pub fn target_of(args: &[&str]) -> Result<Target> {
    let mut given: BTreeMap<&str, &str> = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        if !["--namespace", "--image", "--workspace"].contains(flag) {
            bail!("the dry run takes --image, --workspace and --namespace, not `{flag}`");
        }
        let value = rest
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        if given.insert(flag, value).is_some() {
            bail!("{flag} is given twice");
        }
    }
    let image = given.get("--image").context("the dry run needs --image")?;
    let pinned = image.split_once("@sha256:").is_some_and(|(name, digest)| {
        !name.is_empty()
            && digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    });
    if !pinned {
        bail!("--image {image} is not pinned by a SHA-256 digest");
    }
    let workspace = given
        .get("--workspace")
        .context("the dry run needs --workspace")?;
    let crockford = |b: u8| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b));
    if workspace.len() != 26 || !workspace.bytes().all(crockford) {
        bail!("--workspace {workspace} is not a workspace's ULID");
    }
    Ok(Target {
        namespace: given
            .get("--namespace")
            .unwrap_or(&STAGING_NAMESPACE)
            .to_string(),
        image: image.to_string(),
        workspace: workspace.to_string(),
    })
}

/// The `helm template` arguments a dry run renders the gestures' first container with for
/// `target`: staging's values ([`STAGING_VALUES`]), the target's image as `images.mandate`, and no
/// cluster lookup (the k3d cell's API-server egress comes from its own cluster, which a dry run
/// never reads).
pub fn dry_run_args(target: &Target, id: &str, first: &Gesture) -> Vec<String> {
    template_args(
        &target.namespace,
        STAGING_VALUES,
        &[format!("images.mandate={}", target.image)],
        &[
            format!("run.id=\"{id}\""),
            format!("run.args={}", json!(first.args)),
        ],
        Some("templates/run.yaml"),
    )
}

/// Every object of `objects`, in `namespace`: what a dry run prints, so the objects say where they
/// would be applied.
pub fn in_namespace(mut objects: Vec<Value>, namespace: &str) -> Vec<Value> {
    for object in &mut objects {
        object["metadata"]["namespace"] = json!(namespace);
    }
    objects
}

/// `cargo xtask cell demo --scenario <name> --dry-run <target>`: renders the demo's gestures' Job
/// and its inputs' ConfigMap for `target` ([`target_of`]) with `helm template` and prints them, one
/// JSON object a line, on stdout. It reads no kubeconfig, starts no cell, and applies nothing.
pub fn dry_run(name: &str, args: &[&str]) -> Result<()> {
    if !SCENARIOS.contains(&name) {
        bail!(
            "no scenario `{name}`; the scenarios are {}",
            SCENARIOS.join(", ")
        );
    }
    let target = target_of(args)?;
    let id = run_id()?;
    let inputs = demo_inputs()?;
    let steps = plan(&target.workspace, inputs.envelopes(), None);
    let all = gestures(&steps);
    let first = all.first().context("the scenario has no gesture")?;
    let rendered = render_with(&dry_run_args(&target, &id, first))?;
    let objects = gestures_job(
        rendered,
        &all,
        &format!("demo-in-{id}"),
        &inputs.files(),
        &target.namespace,
    )?;
    for object in in_namespace(objects, &target.namespace) {
        println!("{object}");
    }
    eprintln!(
        "==> the demo's gestures, rendered for {} in workspace {} (dry run: nothing applied)",
        target.namespace, target.workspace
    );
    Ok(())
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
    let inputs = demo_inputs()?;
    let connected = if let ImageSource::Pull(_) = host_image_source()? {
        eprintln!(
            "    connect check: not run: a cell that pulls its images has no executor or \
             deployment manager image, so steps 2 and 7 name their gaps"
        );
        None
    } else {
        let account = account_of(&id);
        let checked = match connect_check(&format!("{id}c"), &workspace, &account)? {
            Ok(checked) => checked,
            Err(logs) => bail!("the executor's connect check for the demo did not pass:\n{logs}"),
        };
        eprintln!("    connect check: passed as {checked} for account {account}");
        Some(Connected { account, checked })
    };
    let steps = plan(&workspace, inputs.envelopes(), connected.as_ref());
    let all = gestures(&steps);
    let first = all.first().context("the scenario has no gesture")?;
    let rendered = render(
        &[
            format!("run.id=\"{id}\""),
            format!("run.args={}", json!(first.args)),
        ],
        Some("templates/run.yaml"),
    )?;
    let files = inputs.files();
    let objects = gestures_job(rendered, &all, &format!("demo-in-{id}"), &files, NAMESPACE)?;
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
    let mut browsed: Option<WebOutcome> = None;
    let mut traded: Option<TradeOutcome> = None;
    for step in &steps {
        let mut previous: Option<&Gesture> = None;
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
                    previous = Some(g);
                }
                Part::Executor(check) => {
                    let (word, said) = if failed.is_some() {
                        ("not run", "an earlier part failed".to_owned())
                    } else {
                        match executor_live(&check.account)? {
                            Ok(pod) => ("ran", format!("{pod} answers livez")),
                            Err(why) => {
                                failed.get_or_insert_with(|| why.clone());
                                ("failed", why)
                            }
                        }
                    };
                    eprintln!("    {word:<8} {}: {said}", check.shows);
                }
                Part::Applied(check) => {
                    let command =
                        previous.and_then(|g| committed_event(&lines_of(&logs, &g.container)));
                    let (word, said) = match (&failed, command) {
                        (Some(_), _) => ("not run", "an earlier part failed".to_owned()),
                        (None, None) => {
                            let why = "the gesture before it named no committed event".to_owned();
                            failed = Some(why.clone());
                            ("failed", why)
                        }
                        (None, Some(command)) => {
                            let read = applied_read(&workspace, check.agent, &command);
                            let (word, said, fails) = poll_applied(&read)?;
                            if fails {
                                failed.get_or_insert_with(|| format!("GET {}: {said}", read.path));
                            }
                            (word, format!("command {command}: {said}"))
                        }
                    };
                    eprintln!("    {word:<8} {}: {said}", check.shows);
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
                Part::WebTradeLoop(check) => {
                    let outcome = match (&failed, &traded) {
                        (Some(_), _) => TradeOutcome::AfterFailure,
                        (None, Some(seen)) => seen.clone(),
                        (None, None) => {
                            let (seen, web) = browse(check.agent, &workspace, &id, &files)?;
                            browsed = Some(web);
                            traded = Some(seen.clone());
                            seen
                        }
                    };
                    let required = Required {
                        cell: env::var_os(CELL_REQUIRED_ENV).is_some(),
                        web: env::var_os(WEB_REQUIRED_ENV).is_some(),
                    };
                    let (word, said, fails) = trade_verdict(&outcome, required);
                    eprintln!("    {word:<8} {}: {said}", check.shows);
                    if fails {
                        failed.get_or_insert_with(|| format!("web/ trade loop: {said}"));
                    }
                }
                Part::Web(check) | Part::WebSignIn(check) => {
                    let outcome = match (&failed, &browsed) {
                        (Some(_), _) => WebOutcome::AfterFailure,
                        (None, Some(seen)) => seen.clone(),
                        (None, None) => {
                            let (seen, web) = browse(check.agent, &workspace, &id, &files)?;
                            traded = Some(seen);
                            browsed = Some(web.clone());
                            web
                        }
                    };
                    let required = Required {
                        cell: env::var_os(CELL_REQUIRED_ENV).is_some(),
                        web: env::var_os(WEB_REQUIRED_ENV).is_some(),
                    };
                    let (word, said, fails) = match part {
                        Part::WebSignIn(_) => signin_verdict(&outcome, &workspace, required),
                        _ => web_verdict(&outcome, check, &workspace, required),
                    };
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

/// Reads `check` with the owner's session every two seconds until it passes, up to
/// [`APPLIED_POLLS`] times: a runtime applies a kill switch on its next pass after it starts,
/// which may be after the Job ends. An API that cannot be asked fails it, since the cell ran.
fn poll_applied(check: &ApiCheck) -> Result<(&'static str, String, bool)> {
    let mut verdict = ("failed", "never read".to_owned(), true);
    for _ in 0..APPLIED_POLLS {
        let answer = api_answer(check)?;
        verdict = api_verdict(&answer, check, true);
        if !verdict.2 {
            break;
        }
        sleep(Duration::from_secs(2));
    }
    Ok(verdict)
}

/// How many times [`poll_applied`] reads, two seconds apart.
const APPLIED_POLLS: u32 = 45;

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
pub(super) fn point_api(workspace: &str) -> Result<()> {
    let hosted = format!("api.workspace={workspace}");
    let upgraded = helm(&[
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
    ]);
    if upgraded.is_err() {
        api_diagnosis();
    }
    upgraded?;
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
#[derive(Debug, Clone)]
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

/// How the browser's trade loop read: as [`WebOutcome`] reads a gesture, or what it reached.
#[derive(Debug, Clone)]
pub enum TradeOutcome {
    AfterFailure,
    /// Lane W's spec is not in this checkout yet (#2462).
    NoSpec,
    Unaskable(String),
    NoTools(String),
    Failed(String),
    /// The loop stopped before any approval reached `web/`, and the API, read with the owner's
    /// session, lists no approval of the agent in any state: the runtime proposed nothing, which
    /// waits on E15-14's composer, not on `web/`.
    Unproposed(String),
    Ran(web::TradeLoop),
}

/// The scenario's one pass through the browser on `agent`, the second agent, which nothing has
/// stopped yet: first the trade loop (lane W's spec, its confirmation run on the host as a control
/// gesture), then, unless the loop pressed the agent's kill switch itself, the kill switch gesture
/// alone, so step 1's sign-in, step 4's loop and step 7's kill switch read one browser session's
/// work. A loop that pressed the kill switch hands step 7 that command.
fn browse(
    agent: &'static str,
    workspace: &str,
    id: &str,
    files: &[(&str, String)],
) -> Result<(TradeOutcome, WebOutcome)> {
    if let Some(why) = api_unaskable()? {
        return Ok((
            TradeOutcome::Unaskable(why.clone()),
            WebOutcome::Unaskable(why),
        ));
    }
    if let Some(why) = web::unavailable()? {
        return Ok((TradeOutcome::NoTools(why.clone()), WebOutcome::NoTools(why)));
    }
    if let Err(why) = api_port_forward(stub_image_tag()?.is_some()) {
        let why = format!("{why:#}");
        return Ok((
            TradeOutcome::Unaskable(why.clone()),
            WebOutcome::Unaskable(why),
        ));
    }
    let mut confirm = |asked: &web::ConfirmRequest| confirm_on_host(asked, id, files);
    let traded = if !web::trade_loop_spec_present()? {
        TradeOutcome::NoSpec
    } else {
        match web::trade_loop_from_web(workspace, agent, &mut confirm) {
            Ok(traded) if traded.first_missing() == Some("approval") => {
                let listed = api_answer(&requests_read(workspace))?;
                unproposed(traded, &listed, agent)
            }
            Ok(traded) => TradeOutcome::Ran(traded),
            Err(why) => TradeOutcome::Failed(format!("{why:#}")),
        }
    };
    let pressed = match &traded {
        TradeOutcome::Ran(t) => t
            .reached
            .get("kill_switch")
            .filter(|c| !c.is_empty())
            .cloned(),
        _ => None,
    };
    let web = match pressed {
        Some(command) => WebOutcome::Recorded {
            agent: agent.to_owned(),
            workspace: workspace.to_owned(),
            read: api_answer(&command_read(workspace, &command))?,
            command,
        },
        None => web_outcome(&WebCheck { shows: "", agent }, workspace)?,
    };
    Ok((traded, web))
}

/// The signed-in read of every approval in `workspace`, whatever its state.
pub fn requests_read(workspace: &str) -> ApiCheck {
    ApiCheck {
        shows: "the workspace's approvals, read with the owner's session",
        path: format!("/v1/workspaces/{workspace}/approvals"),
        status: 200,
        ask: Ask::Session,
        body: Body::Any,
    }
}

/// A loop that stopped before its approval, read against the API's list of approvals: pending as
/// [`TradeOutcome::Unproposed`] only when the API answered 200 and lists none of `agent`'s, in any
/// state. Otherwise the loop stays [`TradeOutcome::Ran`], which fails: a request the runtime made
/// that `web/` never showed is `web/`'s failure, and a list that cannot be read proves nothing.
pub fn unproposed(
    traded: web::TradeLoop,
    listed: &Result<(u16, String), String>,
    agent: &str,
) -> TradeOutcome {
    let none_listed = match listed {
        Ok((200, body)) => serde_json::from_str::<Value>(body).ok().and_then(|v| {
            v["approvals"]
                .as_array()
                .map(|a| !a.iter().any(|x| x["agent_id"] == agent))
        }),
        _ => None,
    };
    if none_listed == Some(true) {
        let why = traded.error.unwrap_or_else(|| "no error named".into());
        TradeOutcome::Unproposed(why)
    } else {
        TradeOutcome::Ran(traded)
    }
}

/// The host's `mandate step-up confirm` for the browser's request, as a one-gesture Job of the
/// `control` process against the cell's journal and store; `Err` names why it did not confirm.
fn confirm_on_host(
    asked: &web::ConfirmRequest,
    id: &str,
    files: &[(&str, String)],
) -> std::result::Result<(), String> {
    let gesture = Gesture {
        container: "g99-step-up-confirm".into(),
        shows: "the host's confirmation of the browser's step-up code",
        command: None,
        args: web::confirm_args(asked, JOURNAL, STORE),
        expect: "confirmed ".into(),
    };
    let run = || -> Result<(bool, String)> {
        let rendered = render(
            &[
                format!("run.id=\"{id}s\""),
                format!("run.args={}", json!(gesture.args)),
            ],
            Some("templates/run.yaml"),
        )?;
        run_job(gestures_job(
            rendered,
            &[&gesture],
            &format!("demo-in-{id}s"),
            files,
            NAMESPACE,
        )?)
    };
    match run() {
        Ok((true, logs))
            if lines_of(&logs, &gesture.container)
                .iter()
                .any(|l| l.starts_with("confirmed ")) =>
        {
            Ok(())
        }
        Ok((_, logs)) => Err(format!(
            "`mandate step-up confirm` did not confirm:\n{logs}"
        )),
        Err(why) => Err(format!("{why:#}")),
    }
}

/// How step 4's trade loop reads: `ran` when every step was reached; `pending` when it stopped
/// before any approval request reached web/, which is the loop's upstream (the runtime's
/// proposal) not being on `main` yet, never a fault of what is; `failed` when it stopped at any
/// later step, or the run itself failed; and, as [`web_verdict`] reads them, a machine without
/// web/'s tools or an API that cannot be asked.
pub fn trade_verdict(outcome: &TradeOutcome, required: Required) -> (&'static str, String, bool) {
    match outcome {
        TradeOutcome::AfterFailure => ("not run", "an earlier part failed".into(), false),
        TradeOutcome::NoSpec => (
            "pending",
            "lane W's web/e2e-cell/trade-loop.spec.ts is not on main yet (#2462)".into(),
            false,
        ),
        TradeOutcome::Unaskable(why) if required.cell => ("failed", why.clone(), true),
        TradeOutcome::NoTools(why) if required.web => ("failed", why.clone(), true),
        TradeOutcome::Unaskable(why) | TradeOutcome::NoTools(why) => {
            ("not run", why.clone(), false)
        }
        TradeOutcome::Failed(why) => ("failed", why.clone(), true),
        TradeOutcome::Ran(t) if t.whole() => (
            "ran",
            format!(
                "approval {}, approved {}, kill switch {}",
                t.reached["approval"], t.reached["approved"], t.reached["kill_switch"]
            ),
            false,
        ),
        TradeOutcome::Unproposed(why) => (
            "pending",
            format!(
                "no approval request reached web/ ({why}), and the API lists none of the agent's: \
                 the runtime proposed nothing, which waits on E15-14's composer"
            ),
            false,
        ),
        TradeOutcome::Ran(t) => {
            let stopped = t.first_missing().unwrap_or("the end");
            let why = t.error.clone().unwrap_or_else(|| "no error named".into());
            ("failed", format!("stopped before {stopped}: {why}"), true)
        }
    }
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
/// stream and reads it accepted. It does not require `applied`: the browser's agent is killed
/// after the Job, and step 7's [`AppliedCheck`] of the CLI's kill switch proves the runtime applies
/// one (DEC-2379).
pub fn command_read(workspace: &str, command: &str) -> ApiCheck {
    ApiCheck {
        shows: "the web gesture's command, read back",
        path: format!("/v1/workspaces/{workspace}/commands/{command}"),
        status: 200,
        ask: Ask::Session,
        body: Body::Command {
            id: command.to_owned(),
            stream: format!("ctl:{workspace}"),
            phases: &COMMAND_PHASES,
            step: None,
        },
    }
}

/// The signed-in read of `command` in `workspace` that passes only once the command is
/// `applied` with a `KillSwitchActivated` step on `agent`'s own stream, which only that agent's
/// runtime writes (the executor writes the account stream), so the read names the process that
/// applied it (DEC-2379, #2430's review).
pub fn applied_read(workspace: &str, agent: &str, command: &str) -> ApiCheck {
    ApiCheck {
        shows: "the command, read back applied",
        path: format!("/v1/workspaces/{workspace}/commands/{command}"),
        status: 200,
        ask: Ask::Session,
        body: Body::Command {
            id: command.to_owned(),
            stream: format!("ctl:{workspace}"),
            phases: &["applied"],
            step: Some(("KillSwitchActivated", format!("agent:{workspace}:{agent}"))),
        },
    }
}

/// The event id a gesture's `committed as event <id>` line names.
pub fn committed_event(lines: &[&str]) -> Option<String> {
    lines.iter().find_map(|l| {
        let (_, rest) = l.split_once("committed as event ")?;
        rest.split_whitespace().next().map(str::to_owned)
    })
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

/// How step 1's web sign-in reads from the browser gesture's outcome, which step 7's kill switch
/// shares (one Playwright run): `ran` once the gesture recorded anything in `workspace`, since it
/// records only after its page, signed in through the stub issuer, read that workspace's agents
/// with a 2xx (`web/e2e-cell/kill-switch.spec.ts`); otherwise as [`web_verdict`] reads a run that
/// did not record.
pub fn signin_verdict(
    outcome: &WebOutcome,
    workspace: &str,
    required: Required,
) -> (&'static str, String, bool) {
    match outcome {
        WebOutcome::Recorded {
            workspace: acted, ..
        } if acted == workspace => (
            "ran",
            format!("web/ read GET /v1/workspaces/{workspace}/agents with its own session"),
            false,
        ),
        WebOutcome::Recorded {
            workspace: acted, ..
        } => (
            "failed",
            format!("web/ read {acted}, not {workspace}"),
            true,
        ),
        other => {
            let check = WebCheck {
                shows: "",
                agent: WEB_AGENT,
            };
            web_verdict(other, &check, workspace, required)
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

    /// The connect check a run's plan names: run 1's account, and a passing check's event id.
    fn connected() -> Connected {
        Connected {
            account: account_of("1"),
            checked: "01KDEC0000000000000000000C".into(),
        }
    }

    /// The plan for `workspace` on the demo's three envelopes, after [`connected`]'s check.
    fn planned(workspace: &str) -> Vec<Step> {
        plan(
            workspace,
            (&mandate(), &web_mandate(), &research_mandate()),
            Some(&connected()),
        )
    }

    /// The plan the staging dry run renders, which names no connect check.
    fn unconnected(workspace: &str) -> Vec<Step> {
        plan(
            workspace,
            (&mandate(), &web_mandate(), &research_mandate()),
            None,
        )
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
        assert_eq!(web["name"], WEB_ENVELOPE);
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
        assert_eq!(
            record_code(
                "01J0ACC0VNT000000000000000",
                "10000000000000000000000001",
                "user-owner"
            ),
            "e92fb7e5",
            "the code `connection record` showed for an Alpaca paper record of the demo's \
             connection, scope trading, on that account, caused by that check, for that owner"
        );
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

    /// The steps run 1 to 7 in order, then the record; a shown code precedes the gesture that types
    /// it, and the typed code is the one shown; every gesture's container name is unique and a
    /// Kubernetes name.
    #[test]
    fn the_plan_runs_its_steps_and_types_the_codes_it_shows() {
        let steps = planned("demo1");
        let numbers: Vec<u8> = steps.iter().map(|s| s.number).collect();
        assert_eq!(
            numbers,
            [1, 2, 3, 4, 5, 6, 7, 8],
            "the seven steps, then the record"
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
        assert_eq!(
            lanes(step),
            ["DR"],
            "only its own connection, so the manager starts its runtime"
        );
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

    /// Step 6 commits the owner's exit of the instrument the demo's envelope pins, on the demo's
    /// agent, at a confirmed bid whose floor is at or below it, its code shown first and then
    /// typed; after the agent is deployed and before the kill switch (a stopped agent's exits are
    /// held, rule 13) and the export, so the closing verify covers it.
    #[test]
    fn step_six_exits_the_pinned_instrument_at_a_confirmed_floor_before_the_kill_switch() {
        let pinned = parsed(INSTRUMENT_FILE);
        assert_eq!(
            pinned["instrument_id"], SPY,
            "the exit names the pinned instrument"
        );
        let scaled = |decimal: &str| -> u128 {
            let (whole, fraction) = decimal.split_once('.').unwrap_or((decimal, ""));
            assert!(
                fraction.len() <= 8,
                "{decimal}: more places than the oracle reads"
            );
            format!("{whole}{fraction:0<8}").parse().unwrap()
        };
        let (bid, size, floor) = EXIT_BID;
        assert!(
            scaled(floor) <= scaled(bid),
            "the floor is at or below the bid"
        );
        assert!(
            scaled(size) > 0 && scaled(floor) > 0,
            "a positive size and floor"
        );
        assert!(scaled("500") < scaled("501.25") && scaled("0.5") > scaled("0.25"));
        let steps = planned("demo1");
        let all = gestures(&steps);
        let at = |head: &[&str], last: &str| {
            all.iter()
                .position(|g| g.args[..head.len()] == *head && g.args.iter().any(|a| a == last))
                .unwrap()
        };
        let exits: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(_, g)| g.args[..2] == ["agent", "exit"])
            .map(|(n, _)| n)
            .collect();
        assert_eq!(exits.len(), 1, "one exit gesture: committed, never shown");
        let typed = exits[0];
        assert_eq!(all[typed].args[2..4], [AGENT, SPY]);
        for pair in [
            ["--confirm-bid", EXIT_BID.0],
            ["--bid-size", EXIT_BID.1],
            ["--floor", EXIT_BID.2],
        ] {
            assert!(all[typed].args.windows(2).any(|w| w == pair), "{pair:?}");
        }
        assert!(
            !all[typed]
                .args
                .iter()
                .any(|a| a == "--code" || a == "--show-code"),
            "an owner's exit reduces risk and takes no code (AGENTS.md rules 2 and 13)"
        );
        assert_eq!(
            all[typed].expect,
            format!("owner_exit {AGENT} {SPY} committed as event")
        );
        let deployed = at(&["agent", "deploy", AGENT], "--code");
        assert!(deployed < typed);
        assert!(
            typed < at(&["agent", "kill"], AGENT),
            "before the kill switch"
        );
        assert!(
            typed < at(&["journal", "export"], SEGMENT),
            "before the export"
        );
        let step6 = &steps[5];
        assert_eq!(step6.number, 6);
        assert!(
            step6
                .parts
                .iter()
                .any(|p| matches!(p, Part::Gesture(g) if g.container == all[typed].container)),
            "the exit is step 6's"
        );
    }

    /// Step 7 commits the owner's kill switch for the demo's agent, at agent scope only (an
    /// agent-scoped switch never reaches the connection or the workspace, rule 13), after the
    /// agent is deployed and before the record is exported, so the closing verify covers it.
    #[test]
    fn step_seven_kills_the_demo_agent_at_agent_scope_before_the_record() {
        let steps = planned("demo1");
        let all = gestures(&steps);
        let at = |head: &[&str]| all.iter().position(|g| g.args[..head.len()] == *head);
        let kill = at(&["agent", "kill"]).expect("step 7 runs `agent kill`");
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
        let step7 = &steps[6].parts;
        assert!(
            step7
                .iter()
                .any(|p| matches!(p, Part::Gesture(x) if x.container == g.container)),
            "the kill switch is step 7's"
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
    /// step 5's backtest is recorded (the research agent deploys in its own workspace, in step 3).
    /// `agent deploy` and `backtest record` each take only the workspace's latest confirmed
    /// version, so its confirmation comes after the record and its deployment after its
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
        assert_eq!(step_of(&all[register].container), Some(7));
        assert_eq!(step_of(&all[deploy].container), Some(7));
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
        assert_eq!(web[0].0, 7, "web/'s kill switch is step 7's");
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
        let mut backlog = fs::read_to_string("../docs/project/06-backlog-v1.md").unwrap();
        let mut rows: Vec<_> = fs::read_dir("../docs/project/backlog")
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect();
        rows.sort();
        for row in rows {
            backlog.push('\n');
            backlog.push_str(&fs::read_to_string(row).unwrap());
        }
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

    /// Which steps run today: the sign-in, through the API and web/, runs whole; the gestures on
    /// `main` make steps 2 to 6 partial, step 3's research gestures among them; the kill switch,
    /// applied by the runtime step 2's deployment starts, and the record run whole. Without a
    /// connect check (the staging dry run) step 7 is partial.
    #[test]
    fn each_step_reads_ready_partial_or_skipped_from_its_parts() {
        let steps = planned("demo1");
        let read: Vec<Readiness> = steps.iter().map(readiness).collect();
        assert_eq!(
            read,
            [
                Ready, Partial, Partial, Partial, Partial, Partial, Ready, Ready
            ]
        );
        let dry: Vec<Readiness> = unconnected("demo1").iter().map(readiness).collect();
        assert_eq!(
            dry,
            [
                Ready, Partial, Partial, Partial, Partial, Partial, Partial, Ready
            ]
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

    /// The slices a wait has named that are on `main`, with the PR that landed each: a lane that
    /// lands one adds its row here, so no wait can go on saying it is still to come.
    const LANDED_SLICES: &[(&str, u32)] = &[("X1a", 1596), ("X1b", 1736)];

    /// The slices `text` names as still to land: each word that follows `slice `, up to the
    /// first character that is not a letter, a digit or a hyphen, so `X1b-3` is not `X1b`.
    fn slices_named(text: &str) -> Vec<&str> {
        text.match_indices("slice ")
            .map(|(at, word)| {
                let rest = &text[at + word.len()..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                    .unwrap_or(rest.len());
                &rest[..end]
            })
            .filter(|slice| !slice.is_empty())
            .collect()
    }

    #[test]
    fn slices_named_reads_whole_slice_ids() {
        assert_eq!(
            slices_named("slice X1b-3: fills; slice X1b, then slice "),
            ["X1b-3", "X1b"]
        );
    }

    /// No wait says it still lands a slice that is on `main`, or waits on a PR that landed one
    /// (#1777's review: X1b was named as to come after #1736 landed it), and every landed slice's
    /// PR is cited in some gap's `on_main`, so what landed is said where the gap stands.
    #[test]
    fn no_wait_names_a_slice_already_on_main() {
        let steps = planned("demo1");
        let gaps: Vec<&Gap> = steps
            .iter()
            .flat_map(|step| &step.parts)
            .filter_map(|part| match part {
                Part::Gap(gap) => Some(gap),
                _ => None,
            })
            .collect();
        for gap in &gaps {
            for wait in &gap.waits {
                for slice in slices_named(wait.lands) {
                    assert!(
                        !LANDED_SLICES.iter().any(|(landed, _)| *landed == slice),
                        "{}: {} still lands slice {slice}, which is on main",
                        gap.what,
                        wait.named()
                    );
                }
                assert!(
                    !LANDED_SLICES.iter().any(|(_, pr)| wait.pr == Some(*pr)),
                    "{}: {} waits on a PR that is on main",
                    gap.what,
                    wait.named()
                );
            }
        }
        for (slice, pr) in LANDED_SLICES {
            let cited = format!("#{pr}");
            assert!(
                gaps.iter().any(|gap| gap.on_main.contains(&cited)),
                "slice {slice} landed as {cited}, but no gap's on_main cites it"
            );
        }
    }

    /// DEC-2379: step 2 records the paper connection the connect check left before anything else,
    /// its code shown and then typed, so the deployment is written at version 2; the executor the
    /// manager starts for that account is checked after the Job. The dry run, with no connect
    /// check, names the gap instead.
    #[test]
    fn step_two_records_the_checked_connection_before_the_deployment() {
        let steps = planned("demo1");
        let all = gestures(&steps);
        let c = connected();
        let user = FOUNDER.to_ascii_lowercase();
        let at = |head: &[&str], with: &str| {
            all.iter()
                .position(|g| g.args[..head.len()] == *head && g.args.iter().any(|a| a == with))
                .unwrap()
        };
        let shown = all
            .iter()
            .position(|g| g.args[..2] == ["connection", "record"])
            .unwrap();
        let typed = at(&["connection", "record"], "--code");
        let code = record_code(&c.account, &c.checked, &user);
        assert_eq!(all[shown].expect, format!("code {code}"));
        assert!(
            all[typed]
                .args
                .windows(2)
                .any(|w| w == ["--code", code.as_str()])
        );
        assert_eq!(all[typed].expect, format!("recorded {CONNECTION} as event"));
        for pair in [
            ["--environment", "paper"],
            ["--connection", CONNECTION],
            ["--broker", "alpaca"],
            ["--scope", "trading"],
            ["--account-ref", c.account.as_str()],
            ["--checked", c.checked.as_str()],
            ["--workspace", "demo1"],
            ["--user", user.as_str()],
        ] {
            assert!(
                all[typed].args.windows(2).any(|w| w == pair),
                "{pair:?} in {:?}",
                all[typed].args
            );
        }
        let deployed = at(&["agent", "deploy", AGENT], "--code");
        assert!(
            shown < typed && typed < deployed,
            "recorded before the deployment"
        );
        let step2 = &steps[1];
        assert_eq!(step2.number, 2);
        assert!(matches!(&step2.parts[0], Part::Gesture(g) if g.container == all[shown].container));
        let executors: Vec<&ExecutorCheck> = steps
            .iter()
            .flat_map(|s| &s.parts)
            .filter_map(|p| match p {
                Part::Executor(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(executors.len(), 1);
        assert_eq!(executors[0].account, c.account);
        assert!(matches!(step2.parts.last(), Some(Part::Executor(_))));
        let dry = unconnected("demo1");
        let dry_all = gestures(&dry);
        assert!(dry_all.iter().all(|g| g.args[0] != "connection"));
        assert!(
            dry.iter()
                .flat_map(|s| &s.parts)
                .all(|p| !matches!(p, Part::Executor(_) | Part::Applied(_)))
        );
    }

    /// DEC-2379: step 7's CLI kill switch is followed by the check that the runtime applied it,
    /// which reads the command the gesture before it committed, and passes on `applied` alone.
    #[test]
    fn step_seven_reads_the_cli_kill_switch_back_applied() {
        let steps = planned("demo1");
        let step7 = &steps[6].parts;
        let at = step7
            .iter()
            .position(|p| matches!(p, Part::Applied(_)))
            .expect("step 7 checks the kill switch applied");
        assert!(
            matches!(&step7[at - 1], Part::Gesture(g) if g.args[..2] == ["agent", "kill"]),
            "the gesture before it is the kill switch"
        );
        assert_eq!(
            committed_event(&[
                "x",
                "kill_switch agent demo-spy committed as event 1TSG2CY1PW at seq 15"
            ]),
            Some("1TSG2CY1PW".into())
        );
        assert_eq!(committed_event(&["code 1234abcd"]), None);
        assert!(
            matches!(&step7[at], Part::Applied(c) if c.agent == AGENT),
            "the runtime of the agent the CLI killed"
        );
        let read = applied_read("demo1", AGENT, "1TSG");
        assert_eq!(read.path, "/v1/workspaces/demo1/commands/1TSG");
        let with_steps = |phase: &str, steps: Value| {
            json!({"command_id": "1TSG", "phase": phase, "steps": steps,
                "as_of": [{"stream_id": "ctl:demo1"}]})
            .to_string()
        };
        let runtime =
            json!([{"stream_id": "agent:demo1:demo-spy", "event_type": "KillSwitchActivated"}]);
        let body = |phase: &str| with_steps(phase, runtime.clone());
        assert_eq!(read.body.missing(&body("applied")), None);
        for phase in ["recorded", "taken", "ended", "refused"] {
            assert!(
                read.body.missing(&body(phase)).is_some(),
                "{phase} is not applied"
            );
        }
        for other in [
            json!([]),
            json!([{"stream_id": "acct:demo1:01KDEA0000000001000000000A", "event_type": "KillSwitchActivated"}]),
            json!([{"stream_id": "agent:demo1:agt_01KDEM00000000000000000WEB", "event_type": "KillSwitchActivated"}]),
            json!([{"stream_id": "agent:demo1:demo-spy", "event_type": "AgentModeApplied"}]),
        ] {
            assert!(
                read.body
                    .missing(&with_steps("applied", other.clone()))
                    .is_some(),
                "applied without the demo agent's runtime's KillSwitchActivated: {other}"
            );
        }
        assert_eq!(
            command_read("demo1", "1TSG").body.missing(&body("taken")),
            None,
            "the web read still accepts any accepted phase"
        );
    }

    /// DR-3: step 1 reads web/'s sign-in from the browser gesture step 7's kill switch also reads,
    /// one Playwright run: `ran` for any recorded gesture in the run's workspace, whatever its
    /// command reads, since the page records only after its signed-in read of the agents; a
    /// gesture in another workspace fails; and a run that recorded nothing reads as step 7's does.
    #[test]
    fn step_one_reads_the_web_sign_in_from_the_shared_browser_gesture() {
        let steps = planned("demo1");
        let sign_in_checks: Vec<(u8, &WebCheck)> = steps
            .iter()
            .flat_map(|s| s.parts.iter().map(move |p| (s.number, p)))
            .filter_map(|(n, p)| match p {
                Part::WebSignIn(c) => Some((n, c)),
                _ => None,
            })
            .collect();
        assert_eq!(sign_in_checks.len(), 1);
        assert_eq!(sign_in_checks[0].0, 1, "the sign-in is step 1's");
        let web: Vec<&WebCheck> = steps
            .iter()
            .flat_map(|s| &s.parts)
            .filter_map(|p| match p {
                Part::Web(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(
            sign_in_checks[0].1.agent, web[0].agent,
            "one gesture, one agent"
        );
        let ws = "01KDEM0000000001000000000D";
        let recorded = |acted: &str, read: Result<(u16, String), String>| WebOutcome::Recorded {
            agent: WEB_AGENT.into(),
            workspace: acted.into(),
            command: "C1".into(),
            read,
        };
        for cell in [false, true] {
            for web in [false, true] {
                let required = Required { cell, web };
                for read in [Ok((200, "{}".to_owned())), Err("no answer".to_owned())] {
                    let (word, _, fails) = signin_verdict(&recorded(ws, read), ws, required);
                    assert_eq!((word, fails), ("ran", false));
                }
                let (word, _, fails) =
                    signin_verdict(&recorded("other", Ok((200, "{}".into()))), ws, required);
                assert_eq!((word, fails), ("failed", true));
                let check = WebCheck {
                    shows: "s",
                    agent: WEB_AGENT,
                };
                for other in [
                    WebOutcome::AfterFailure,
                    WebOutcome::Unaskable("no kubectl".into()),
                    WebOutcome::NoTools("no npm".into()),
                    WebOutcome::Failed("the run failed".into()),
                ] {
                    assert_eq!(
                        signin_verdict(&other, ws, required),
                        web_verdict(&other, &check, ws, required),
                        "{other:?}"
                    );
                }
            }
        }
    }

    /// Step 4 runs lane W's trade loop on the second agent, the one nothing stops before the parts
    /// run (the CLI's kill switch is the demo agent's), and reads it: `ran` only when every step
    /// was reached; `pending`, never failing, only as [`TradeOutcome::Unproposed`], the API's word
    /// that the runtime asked for nothing; and `failed` for a loop that stopped at any step,
    /// approval included, or for a run that failed.
    #[test]
    fn step_four_reads_the_browser_trade_loop() {
        let steps = planned("demo1");
        let loops: Vec<(u8, &WebCheck)> = steps
            .iter()
            .flat_map(|s| s.parts.iter().map(move |p| (s.number, p)))
            .filter_map(|(n, p)| match p {
                Part::WebTradeLoop(c) => Some((n, c)),
                _ => None,
            })
            .collect();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].0, 4);
        assert_eq!(loops[0].1.agent, WEB_AGENT, "never the agent the CLI kills");
        let reached = |steps: &[&str], error: Option<&str>| {
            TradeOutcome::Ran(web::TradeLoop {
                reached: steps
                    .iter()
                    .map(|s| ((*s).to_owned(), format!("v-{s}")))
                    .collect(),
                error: error.map(str::to_owned),
            })
        };
        for required in [
            Required {
                cell: false,
                web: false,
            },
            Required {
                cell: true,
                web: true,
            },
        ] {
            let (word, said, fails) =
                trade_verdict(&reached(&web::TRADE_LOOP_STEPS, None), required);
            assert_eq!((word, fails), ("ran", false), "{said}");
            assert!(said.contains("v-kill_switch"), "{said}");
            let (word, said, fails) =
                trade_verdict(&reached(&[], Some("no approval within 20000 ms")), required);
            assert_eq!(
                (word, fails),
                ("failed", true),
                "a loop left at its approval fails unless the API showed no request: {said}"
            );
            let (word, _, fails) = trade_verdict(
                &TradeOutcome::Unproposed("no approval within 20000 ms".into()),
                required,
            );
            assert_eq!(
                (word, fails),
                ("pending", false),
                "no proposal yet is upstream, not a fault"
            );
            for stopped_at in 0..web::TRADE_LOOP_STEPS.len() {
                let (word, said, fails) = trade_verdict(
                    &reached(&web::TRADE_LOOP_STEPS[..stopped_at], Some("x")),
                    required,
                );
                assert_eq!((word, fails), ("failed", true), "{said}");
                assert!(said.contains(web::TRADE_LOOP_STEPS[stopped_at]), "{said}");
            }
            let (word, _, fails) =
                trade_verdict(&reached(&web::TRADE_LOOP_STEPS, Some("late")), required);
            assert_eq!(
                (word, fails),
                ("failed", true),
                "an error fails a loop that reached every step"
            );
            let (word, _, fails) = trade_verdict(&TradeOutcome::Failed("x".into()), required);
            assert_eq!((word, fails), ("failed", true));
            let (word, _, fails) = trade_verdict(&TradeOutcome::AfterFailure, required);
            assert_eq!((word, fails), ("not run", false));
            let (word, _, fails) = trade_verdict(&TradeOutcome::NoSpec, required);
            assert_eq!(
                (word, fails),
                ("pending", false),
                "the spec not yet on main fails nothing"
            );
        }
        let none = Required {
            cell: false,
            web: false,
        };
        let web_only = Required {
            cell: false,
            web: true,
        };
        let cell_only = Required {
            cell: true,
            web: false,
        };
        let no_npm = TradeOutcome::NoTools("no npm".into());
        assert!(!trade_verdict(&no_npm, none).2);
        assert!(trade_verdict(&no_npm, web_only).2);
        let unaskable = TradeOutcome::Unaskable("no kubectl".into());
        assert!(!trade_verdict(&unaskable, web_only).2);
        assert!(trade_verdict(&unaskable, cell_only).2);
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

    const PINNED: &str = "ghcr.io/kunwarshivam/mandate/mandate@sha256:\
                          0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const STAGING_WORKSPACE: &str = "01KDEM0000000000000000000W";

    /// A dry run's target: staging's namespace unless `--namespace` names another, the image pinned
    /// by a SHA-256 digest, and the fixed workspace's ULID; anything else is refused, so a dry run
    /// never renders a tag admission would refuse or a workspace the API does not host.
    #[test]
    fn a_dry_run_targets_a_pinned_image_and_a_workspace_ulid() {
        let target = target_of(&["--image", PINNED, "--workspace", STAGING_WORKSPACE]).unwrap();
        assert_eq!(
            target,
            Target {
                namespace: STAGING_NAMESPACE.into(),
                image: PINNED.into(),
                workspace: STAGING_WORKSPACE.into(),
            }
        );
        let other = target_of(&[
            "--workspace",
            STAGING_WORKSPACE,
            "--namespace",
            "ws-ws2",
            "--image",
            PINNED,
        ])
        .unwrap();
        assert_eq!(other.namespace, "ws-ws2");
        let tagged = "ghcr.io/kunwarshivam/mandate/mandate:latest";
        let short = "ghcr.io/kunwarshivam/mandate/mandate@sha256:0123abcd";
        let upper = PINNED.to_ascii_uppercase();
        for refused in [
            vec!["--workspace", STAGING_WORKSPACE],
            vec!["--image", PINNED],
            vec!["--image", tagged, "--workspace", STAGING_WORKSPACE],
            vec!["--image", short, "--workspace", STAGING_WORKSPACE],
            vec!["--image", &upper, "--workspace", STAGING_WORKSPACE],
            vec![
                "--image",
                "@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "--workspace",
                STAGING_WORKSPACE,
            ],
            vec!["--image", PINNED, "--workspace", "ws1"],
            vec![
                "--image",
                PINNED,
                "--workspace",
                "01KDEM000000000000000000OW",
            ],
            vec![
                "--image",
                PINNED,
                "--workspace",
                "01kdem0000000000000000000w",
            ],
            vec![
                "--image",
                PINNED,
                "--workspace",
                STAGING_WORKSPACE,
                "--kubeconfig",
                "k",
            ],
            vec![
                "--image",
                PINNED,
                "--workspace",
                STAGING_WORKSPACE,
                "--image",
                PINNED,
            ],
            vec!["--image", PINNED, "--workspace"],
        ] {
            assert!(target_of(&refused).is_err(), "{refused:?} is refused");
        }
    }

    /// A dry run renders with staging's values in its HelmRelease's order, the target's namespace
    /// and image, the first gesture as the run, and nothing read from a cluster: no API-server
    /// egress, which only the k3d cell's own cluster answers.
    #[test]
    fn a_dry_run_renders_with_staging_values_and_reads_no_cluster() {
        let target = target_of(&["--image", PINNED, "--workspace", STAGING_WORKSPACE]).unwrap();
        let steps = planned(&target.workspace);
        let all = gestures(&steps);
        let args = dry_run_args(&target, "7", all[0]);
        let expected: Vec<String> = [
            "template",
            "workspace",
            WORKSPACE_CHART,
            "--namespace",
            STAGING_NAMESPACE,
            "-f",
            "deploy/cell/values/managed.yaml",
            "-f",
            "deploy/gitops/clusters/staging/images.yaml",
            "--set",
            &format!("images.mandate={PINNED}"),
            "--set-json",
            "run.id=\"7\"",
            "--set-json",
            &format!("run.args={}", json!(all[0].args)),
            "--show-only",
            "templates/run.yaml",
        ]
        .iter()
        .map(|a| a.to_string())
        .collect();
        assert_eq!(args, expected);
        assert!(
            !args
                .iter()
                .any(|a| a.contains("kube-api") || a.contains("kubeconfig"))
        );
        for file in STAGING_VALUES {
            assert!(
                Path::new("..").join(file).is_file(),
                "{file} is in the checkout"
            );
        }
        assert!(
            all[0]
                .args
                .windows(2)
                .any(|w| w == ["--workspace", STAGING_WORKSPACE]),
            "the gestures act on the target's workspace"
        );
    }

    /// The inputs' ConfigMap is made in the namespace it is given, and a dry run prints every
    /// object in the target's namespace.
    #[test]
    fn a_dry_run_prints_every_object_in_the_targets_namespace() {
        let steps = planned("demo1");
        let all = gestures(&steps);
        let files = [("mandate.json", "{}".to_string())];
        let objects = gestures_job(rendered(), &all, "demo-in-1", &files, "ws-ws2").unwrap();
        let configmap = objects.iter().find(|o| o["kind"] == "ConfigMap").unwrap();
        assert_eq!(configmap["metadata"]["namespace"], "ws-ws2");
        let printed = in_namespace(objects, "ws-ws2");
        assert!(printed.len() >= 2);
        assert!(
            printed
                .iter()
                .all(|o| o["metadata"]["namespace"] == "ws-ws2")
        );
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
        let objects = gestures_job(rendered(), &all, "demo-in-1", &files, NAMESPACE).unwrap();
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

    /// #2466's review: a loop left at its approval is pending only on the API's word that the
    /// agent has no approval at all. One the runtime requested and `web/` never showed, any state,
    /// or a list that cannot be read, a non-200, or a body that is not the list, all fail.
    #[test]
    fn a_loop_left_at_its_approval_is_pending_only_when_the_api_lists_none_of_the_agent_s() {
        let stopped = || web::TradeLoop {
            reached: BTreeMap::new(),
            error: Some("no approval within 20000 ms".into()),
        };
        let listed = |status: u16, body: &str| Ok((status, body.to_owned()));
        let pending = |outcome: &TradeOutcome| matches!(outcome, TradeOutcome::Unproposed(_));
        let other = r#"{"approvals":[{"agent_id":"other","state":"pending"}]}"#;
        assert!(pending(&unproposed(
            stopped(),
            &listed(200, r#"{"approvals":[]}"#),
            WEB_AGENT
        )));
        assert!(pending(&unproposed(
            stopped(),
            &listed(200, other),
            WEB_AGENT
        )));
        for state in ["pending", "approved", "expired"] {
            let body = json!({"approvals": [{"agent_id": WEB_AGENT, "state": state}]}).to_string();
            assert!(
                !pending(&unproposed(stopped(), &listed(200, &body), WEB_AGENT)),
                "a {state} request was made"
            );
        }
        for answer in [
            listed(503, r#"{"approvals":[]}"#),
            listed(200, "not json"),
            listed(200, r#"{"agents":[]}"#),
            Err("no kubectl".to_owned()),
        ] {
            assert!(
                !pending(&unproposed(stopped(), &answer, WEB_AGENT)),
                "{answer:?}"
            );
        }
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
