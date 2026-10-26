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
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::executor::{CONNECTION, RESEARCH_CONNECTION, connect_check, executor_live};
use super::{
    API_LOCAL_PORT, API_PROCESS, CELL_REQUIRED_ENV, DEMO_OWNER, DEPLOY_MANAGER_PROCESS,
    ImageSource, MODEL_GATEWAY_PROCESS, NAMESPACE, NODE, RESEARCH_MANAGER_PROCESS,
    RESEARCH_SCHEDULER_PROCESS, SCHEDULER_PROCESS, WORKSPACE_CHART, api_diagnosis, api_image_tag,
    api_port_forward, helm, host_image_source, http, kubectl, open_session, processes, render,
    render_with, reply_body, run_id, run_job, session_cookie, signed_in, stub_image_tag,
    stub_token, template_args, tools_missing, up, web,
};
use crate::output;

/// The scenarios `--scenario` takes.
pub const SCENARIOS: &[&str] = &["demo"];
/// The demo's mandate, its instrument snapshot, and where each is mounted in the gestures' pod.
/// Both are stored as canonical JSON (journal spec §4), so a version is the SHA-256 of the file.
pub const MANDATE_FILE: &str = "deploy/cell/demo/mandate.json";
pub const INSTRUMENT_FILE: &str = "deploy/cell/demo/instrument.json";
/// The reviewed configuration every agent the demo deploys trades under, registered on the
/// workspace's control stream before any version: the gate's rule set, which the runtime's gate
/// and the executor's binding gate both read as the stream's latest `rule_set` (one artifact for
/// both), and the paper fee schedule over its trading calendar, which the executor reads. Each is
/// the shell's reviewed tracer configuration, as canonical JSON.
pub const RULE_SET_FILE: &str = "deploy/cell/demo/rule-set.json";
pub const FEE_CONFIG_FILE: &str = "deploy/cell/demo/fee-config.json";
pub const TRADING_CALENDAR_FILE: &str = "deploy/cell/demo/trading-calendar.json";
/// The demo's envelope on QQQ and QQQ's snapshot, as canonical JSON: deployed by no step since the
/// run's workspace holds one agent, they stay as `mandate-runtime-models`' test mandate and the
/// data process's second instrument, and the canonical-JSON test still checks them.
#[cfg(test)]
pub const WEB_MANDATE_FILE: &str = "deploy/cell/demo/web-mandate.json";
#[cfg(test)]
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
/// The `executor` drill's agent, in its own workspace.
pub const AGENT: &str = "demo-spy";
/// The instrument the demo's envelope pins: SPY, under its Alpaca asset id ([`INSTRUMENT_FILE`]'s
/// `instrument_id`).
#[cfg(test)]
pub const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
/// The run's workspace's one agent, on the demo's envelope ([`MANDATE_FILE`]): one agent to an
/// account (V-048), and one Alpaca paper connection to a workspace until the account fingerprint
/// tells two apart (DEC-802 item 3). Every step acts on it, the CLI's in the gestures' Job and the
/// browser's after it, so nothing in the Job stops it: its kill switch is the browser's, last. Its
/// id is `agt_` and a ULID, the only agent id web/'s routes open (`web/src/lib/ids.ts`'s
/// `AGENT_ID`); any other id is a 404 there.
pub const WEB_AGENT: &str = "agt_01KDEM00000000000000000WEB";
/// [`WEB_MANDATE_FILE`]'s envelope's `name`: a mandate name is lowercase letters, digits and
/// hyphens (`mandate.schema.json`), so it cannot be an agent's id, and nothing requires that it is.
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

/// The research workspace's own account stream for the run (DEC-2247 item 3): a ULID of its own,
/// so its connection binds an account no other workspace's connection does.
pub fn research_account_of(id: &str) -> String {
    format!("01KDER{id:0>10.10}000000000A")
}

/// A paper connection as the executor's connect check left it: the connection, the account it
/// binds and the passing `ConnectionChecked` the record names as its cause.
#[derive(Debug, Clone)]
pub struct Connected {
    pub connection: &'static str,
    pub account: String,
    pub checked: String,
}

/// The run's two paper connections, each from its own connect check: the demo workspace's, which
/// step 2 records, and the research workspace's, which step 3 records there.
#[derive(Debug, Clone)]
pub struct Connections {
    pub demo: Connected,
    pub research: Connected,
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
    Loop(LoopCheck),
    Grant(GrantCheck),
    Gap(Gap),
}

impl Part {
    /// The gap a part names: a [`Part::Gap`] itself, or the gap a link of the research loop stands
    /// for until a run of the cell shows it, so the plan's lanes and readiness count its waits.
    pub fn gap(&self) -> Option<&Gap> {
        match self {
            Part::Gap(gap) => Some(gap),
            Part::Loop(check) => Some(&check.gap),
            Part::Grant(check) => Some(&check.gap),
            _ => None,
        }
    }
}

/// A link of the research workspace's trade loop, read from the journal after the gestures' Job
/// (DEC-2071): an event of `event_type` on `stream` whose stored body holds `holding` when it names
/// one, after the last link seen on that stream, so each link follows the one before it, polled
/// until it is there or the loop's wait is spent. Until a run of the cell shows it, it
/// stands for `gap`, so a link not yet reached is named with the lanes it waits on, never failed.
#[derive(Debug)]
pub struct LoopCheck {
    pub shows: &'static str,
    pub stream: String,
    pub event_type: &'static str,
    pub holding: Option<&'static str>,
    pub gap: Gap,
}

/// The owner's grant of the research agent's pending opening, on the CLI, after the gestures' Job,
/// since no approval exists before the runtime asks: `approvals list` finds it, `approvals show`
/// shows its code, and `approvals approve` types that code (mandate spec §6.4). It stands for `gap`
/// until a run of the cell shows it.
#[derive(Debug)]
pub struct GrantCheck {
    pub shows: &'static str,
    pub workspace: String,
    pub agent: &'static str,
    pub gap: Gap,
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
    let gaps = step.parts.iter().filter(|p| p.gap().is_some()).count();
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
        if let Some(gap) = part.gap() {
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
            if let Some(gap) = part.gap() {
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
    lands: "a check that the research agent's runtime, which the manager starts from its \
            version-2 deployment, journals its first research run over the recorded bundle",
};
const GW_RESEARCH: Wait = Wait {
    lane: "GW",
    story: "E21-62",
    pr: None,
    lands: "a run of the cell that shows the research agent's runtime, ticking on its own \
            workspace's clock (DEC-2071), research over the recorded bundle through the model \
            gateway and propose its thesis",
};
const NE_NEWS: Wait = Wait {
    lane: "NE",
    story: "E19-12",
    pr: None,
    lands: "the news item that ends the thesis reaching the research runtime in the cell, and its \
            monitor run's invalidation (DEC-2394)",
};
const DR_CONNECT: Wait = Wait {
    lane: "DR",
    story: "E21-54",
    pr: None,
    lands: "a connection the dry run can name: staging's comes from the owner's OAuth connect, \
            which the scenario does not run yet",
};
const WB_EXIT: Wait = Wait {
    lane: "WB",
    story: "E11-9",
    pr: None,
    lands: "web/'s owner exit gesture with its bid confirmation, and the trade loop's \
            `owner_exit` step between the protective stop and the kill switch",
};
const W2_WEB: Wait = Wait {
    lane: "W2",
    story: "E11-9",
    pr: None,
    lands: "web/ sends the demo's gestures through the API, and `cell demo` drives web/ against \
            the cell (claim #1443)",
};
const E2E_FILL: Wait = Wait {
    lane: "E2E",
    story: "E21-54",
    pr: None,
    lands: "a gesture that shows the granted opening's fill and resting stop in the cell, in place \
            of this gap",
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
    "sha256:2723d06322272475e568c2111fe113ee4d38b3c6755ba758e65b16dce6e9e1e2";
pub const DEMO_REPORT: &str =
    "sha256:3172f2086db7b356d999110b941f62d7a7ea8614e95ebc9ac2565506f6204fc4";

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
pub fn record_code(connection: &str, account: &str, checked: &str, user: &str) -> String {
    code(&format!(
        concat!(
            r#"{{"account_ref":"{account}","broker":"alpaca","causation_id":"{checked}","#,
            r#""connection_id":"{connection}","environment":"paper","#,
            r#""gesture":"connection_record","margin_attestation":null,"#,
            r#""scopes":["trading"],"user":"{user}"}}"#
        ),
        account = account,
        checked = checked,
        connection = connection,
        user = user,
    ))
}

/// The go-live disclosure the customer journey accepts on the host (DEC-2134), a placeholder
/// pending counsel, as its file holds it.
const GO_LIVE_DISCLOSURE: &str = include_str!("../../../fixtures/disclosures/go_live_risk.json");

/// The go-live disclosure's version: `sha256:` over its canonical JSON (sorted keys, no
/// whitespace), as `mandate disclosure accept` stores it.
///
/// # Errors
/// A file that is not JSON.
pub fn disclosure_version() -> Result<String> {
    let parsed: Value =
        serde_json::from_str(GO_LIVE_DISCLOSURE).context("the go-live disclosure is not JSON")?;
    Ok(reference(serde_json::to_string(&parsed)?.as_bytes()))
}

/// The code `mandate disclosure accept go_live_risk` shows for `version` (DEC-530 item 4).
pub fn disclosure_code(version: &str) -> String {
    code(&format!(
        r#"{{"document":"go_live_risk","gesture":"disclosure_accept","version":"{version}"}}"#
    ))
}

/// The owner accepts the go-live disclosure on the host in `workspace`, as the go-live screen
/// asks, by running `mandate disclosure accept` with the code it shows, in a Job of the demo's
/// image.
///
/// # Errors
/// A Job that did not run, or a CLI that did not print `accepted`.
pub fn accept_disclosure(workspace: &str) -> Result<()> {
    let id = run_id()?;
    let user = FOUNDER.to_ascii_lowercase();
    let code = disclosure_code(&disclosure_version()?);
    let args = [
        "disclosure",
        "accept",
        "go_live_risk",
        "--code",
        &code,
        "--workspace",
        workspace,
        "--user",
        &user,
        "--journal",
        JOURNAL,
        "--store",
        STORE,
    ];
    let gesture = Gesture {
        container: "g98-disclosure-accept".into(),
        shows: "the owner's acceptance of the go-live disclosure, on the host",
        command: None,
        args: args.map(str::to_owned).to_vec(),
        expect: "accepted ".into(),
    };
    let rendered = render(
        &[
            format!("run.id=\"{id}d\""),
            format!("run.args={}", json!(gesture.args)),
        ],
        Some("templates/run.yaml"),
    )?;
    let inputs = demo_inputs()?;
    let (succeeded, logs) = run_job(gestures_job(
        rendered,
        &[&gesture],
        &format!("demo-in-{id}d"),
        &inputs.files(),
        NAMESPACE,
    )?)?;
    let accepted = lines_of(&logs, &gesture.container)
        .iter()
        .any(|l| l.starts_with("accepted "));
    if !(succeeded && accepted) {
        bail!("`mandate disclosure accept` did not accept the go-live disclosure:\n{logs}");
    }
    eprintln!("    disclosure: the owner accepted go_live_risk on the host");
    Ok(())
}

pub(super) fn deploy_code(agent: &str, version: &str) -> String {
    code(&format!(
        r#"{{"agent_id":"{agent}","gesture":"agent_deploy","mandate_version":"{version}"}}"#
    ))
}

/// The demo's steps for `workspace`, given the bytes of the demo's mandate, of
/// [`RESEARCH_AGENT`]'s, and the paper connections the executor's connect checks left
/// `connected`.
pub fn plan(
    workspace: &str,
    (mandate, research_mandate): (&[u8], &[u8]),
    connections: Option<&Connections>,
) -> Vec<Step> {
    let connected = connections.map(|c| &c.demo);
    let version = reference(mandate);
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
    let rule_set_in = format!("{INPUTS}/rule-set.json");
    let fee_in = format!("{INPUTS}/fee-config.json");
    let calendar_in = format!("{INPUTS}/trading-calendar.json");
    let (confirm, deploy) = (confirm_code(&version), deploy_code(WEB_AGENT, &version));
    let (research_confirm, research_deploy) = (
        confirm_code(&research_version),
        deploy_code(RESEARCH_AGENT, &research_version),
    );
    let record_words = |connected: &Connected, extra: &[&str]| -> Vec<String> {
        [
            &[
                "connection",
                "record",
                "--environment",
                "paper",
                "--connection",
                connected.connection,
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
        .concat()
        .iter()
        .map(|a| a.to_string())
        .collect()
    };
    let record_args = |connected: &Connected, extra: &[&str]| {
        let words = record_words(connected, extra);
        owner(&words.iter().map(String::as_str).collect::<Vec<_>>())
    };
    let research_record_args = |connected: &Connected, extra: &[&str]| {
        let words = record_words(connected, extra);
        researcher(&words.iter().map(String::as_str).collect::<Vec<_>>())
    };
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
                    body: Body::Lists(WEB_AGENT),
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
                        let recorded = record_code(connected.connection, &connected.account, &connected.checked, &user);
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
                                format!("recorded {} as event", connected.connection),
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
                        "the gate's reviewed rule set, registered: the one both the runtime's \
                         gate and the executor's read",
                        owner(&["config", "register", "--kind", "rule_set", &rule_set_in]),
                        "registered as event".into(),
                    ),
                    gesture(
                        "the paper fee schedule, registered",
                        owner(&["config", "register", "--kind", "fee_config", &fee_in]),
                        "registered as event".into(),
                    ),
                    gesture(
                        "its trading calendar, registered",
                        owner(&[
                            "config",
                            "register",
                            "--kind",
                            "trading_calendar",
                            &calendar_in,
                        ]),
                        "registered as event".into(),
                    ),
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
                        owner(&["agent", "deploy", WEB_AGENT, &version]),
                        format!("code {deploy}"),
                    ),
                    gesture(
                        "the agent, deployed with the code shown",
                        owner(&["agent", "deploy", WEB_AGENT, &version, "--code", &deploy]),
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
            parts: [
                vec![
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
                ],
                match connections.map(|c| &c.research) {
                    Some(connected) => {
                        let recorded =
                            record_code(connected.connection, &connected.account, &connected.checked, &user);
                        vec![
                            gesture(
                                "the research workspace's own paper connection's record, its \
                                 code shown (nothing committed), naming its own connect check",
                                research_record_args(connected, &[]),
                                format!("code {recorded}"),
                            ),
                            gesture(
                                "that connection, recorded there with the code shown, so the \
                                 research agent's deployment is written at version 2",
                                research_record_args(connected, &["--code", &recorded]),
                                format!("recorded {} as event", connected.connection),
                            ),
                        ]
                    }
                    None => vec![Part::Gap(Gap {
                        what: "the research workspace's own paper connection, recorded from its \
                               connect, so its deployment is written at version 2",
                        on_main: "the executor's connect check and its record in the local cell \
                                  (#1759, #1763), which a run with the executor's image runs for \
                                  each workspace",
                        waits: vec![DR_CONNECT],
                    })],
                },
                vec![
                gesture(
                    "the same reviewed rule set, registered there: the one its runtime's gate and \
                     its executor's read (DEC-2819 item 3)",
                    researcher(&["config", "register", "--kind", "rule_set", &rule_set_in]),
                    "registered as event".into(),
                ),
                gesture(
                    "the paper fee schedule, registered there",
                    researcher(&["config", "register", "--kind", "fee_config", &fee_in]),
                    "registered as event".into(),
                ),
                gesture(
                    "its trading calendar, registered there",
                    researcher(&[
                        "config",
                        "register",
                        "--kind",
                        "trading_calendar",
                        &calendar_in,
                    ]),
                    "registered as event".into(),
                ),
                gesture(
                    "SPY's instrument snapshot, registered there: the one instrument its research \
                     admits, whose own snapshot its executor reads",
                    researcher(&[
                        "config",
                        "register",
                        "--kind",
                        "instrument_snapshot",
                        &instrument_in,
                    ]),
                    "registered as event".into(),
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
                ],
                connections
                    .map(|c| {
                        vec![Part::Executor(ExecutorCheck {
                            shows: "the research workspace's executor, started by the deployment \
                                    manager from the research agent's version-2 deployment, \
                                    running and live",
                            account: c.research.account.clone(),
                        })]
                    })
                    .unwrap_or_default(),
                match connections {
                    Some(c) => research_loop(&research_workspace, &c.research.account),
                    None => vec![Part::Gap(Gap {
                        what: "a research run in the research agent's runtime, over the recorded \
                               bundle its image carries, through mandate-gateway's recorded fake",
                        on_main: "the runtime opted into research over the bundle (#1757), the \
                                  bundle in the interim runtime image (#1769), every managed \
                                  runtime pod handed it (#1774), and the research model's \
                                  registration (#1779, #1782), and the connect check and record \
                                  that write a deployment at version 2 (#1759, #1763)",
                        waits: vec![DR_RESEARCH],
                    })],
                },
            ]
            .into_iter()
            .flatten()
            .collect(),
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
                    owner(&["approvals", "list", "--agent", WEB_AGENT]),
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
                    shows: "the trade loop from web/ on the agent: its approval request \
                            shown, approved with the code the host confirms, the order filled, \
                            the position and the journal shown, then its kill switch",
                    agent: WEB_AGENT,
                }),
                Part::Gap(Gap {
                    what: "the paper fill and the protective stop on a fixture broker",
                    on_main: "the executor process (#1596, binary #1694) and its interim image \
                              in the cell on the fixture paper broker (#1702, DEC-2031), handing \
                              its agents' orders to the core (#1736); the fixture fills, and the \
                              executor reads each deployment's mandate, gate and protection \
                              (#2547, #2605) and its broker by asset id, so a deployed opening is \
                              gated, filled, booked and protected in process (#2624, DEC-2813); \
                              the cell's daemon asks on a deferring opening (#2538), and, in \
                              process, the runtime's production wiring decides, asks and \
                              proposes the opening once granted, on the demo envelope (#2637); \
                              the cell's runtime is wired by the gap above",
                    waits: vec![E2E_FILL],
                }),
            ],
        },
        Step {
            number: 5,
            title: "a backtest of the same mandate version is shown",
            parts: vec![
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
            parts: vec![Part::Gap(Gap {
                what: "the owner's exit of the agent's position from web/, at a confirmed bid, \
                       after the trade loop's fill and its protective stop, so the sale shows in \
                       its positions and audit before the kill switch",
                on_main: "the owner's exit over the API (#2501), handed to the executor's core and \
                          priced at the floor it confirmed (#1736, DEC-2033 item 4); an exit \
                          before the fill sells nothing, so no gesture of the Job runs one",
                waits: vec![WB_EXIT],
            })],
        },
        Step {
            number: 7,
            title: "the kill switch from the web app stops the agent",
            parts: vec![
                Part::Web(WebCheck {
                    shows: "the owner's kill switch for the agent, pressed in web/ and recorded \
                            through the API on the run's control stream: the trade loop's own, \
                            or, when the loop stopped before it, the kill switch gesture alone",
                    agent: WEB_AGENT,
                }),
                match connected {
                    Some(_) => Part::Applied(AppliedCheck {
                        shows: "the agent's runtime, started from step 2's version-2 \
                                deployment, applying web/'s kill switch: its command read back \
                                applied with the owner's session",
                        agent: WEB_AGENT,
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
            | Part::Loop(_)
            | Part::Grant(_)
            | Part::Gap(_) => None,
        })
        .collect()
}

/// The research workspace's trade loop after its executor is live (DEC-2071, DEC-2394), each link
/// its own part, in the order the story runs: the agent's research run and thesis on its own
/// stream, the opening it asks, the owner's grant on the CLI, the runtime's record of it, the
/// paper fill and the protective stop on `account`'s stream, then the news item that ends the
/// thesis, its invalidation, and the thesis exit's sell. Every link until a run of the cell shows
/// it names the lanes it waits on; an opening is decided only in the regular session.
fn research_loop(workspace: &str, account: &str) -> Vec<Part> {
    let agent = format!("agent:{workspace}:{RESEARCH_AGENT}");
    let acct = format!("acct:{workspace}:{account}");
    let link = |shows: &'static str,
                stream: &str,
                event_type: &'static str,
                holding: Option<&'static str>,
                what: &'static str,
                on_main: &'static str,
                waits: Vec<Wait>| {
        Part::Loop(LoopCheck {
            shows,
            stream: stream.to_owned(),
            event_type,
            holding,
            gap: Gap {
                what,
                on_main,
                waits,
            },
        })
    };
    let session = "an opening is decided only in the regular session, 13:30 to 20:00 UTC on a \
                   trading day, so outside it this link waits for the next";
    vec![
        link(
            "the research agent's runtime starts a research run on its own clock",
            &agent,
            "RunStarted",
            None,
            "a research run in the research agent's runtime, over the recorded bundle its image \
             carries, through the model gateway's recorded fake",
            "the runtime calls the model gateway (#2636) and ticks on its own workspace's clock \
             (DEC-2071)",
            vec![GW_RESEARCH],
        ),
        link(
            "its research proposes a thesis, admitted",
            &agent,
            "ThesisProposed",
            Some(r#""admitted":true"#),
            "the research run's thesis, admitted through the eligibility floor and the autonomy \
             rules",
            "the recorded bundle's reply and the core's thesis lifecycle (DEC-2246)",
            vec![GW_RESEARCH],
        ),
        link(
            "the runtime sizes the opening and the gate and autonomy rules ask",
            &agent,
            "ApprovalRequested",
            Some(r#""purpose":"open""#),
            "the opening the runtime sizes for the thesis, asked under the envelope's autonomy \
             rules",
            session,
            vec![P5_HOOK],
        ),
        Part::Grant(GrantCheck {
            shows: "the owner grants it on the CLI, its code shown and then typed",
            workspace: workspace.to_owned(),
            agent: RESEARCH_AGENT,
            gap: Gap {
                what: "the owner's grant of the asked opening",
                on_main: "`mandate approvals list`, `show` and `approve` (K1a, DEC-533)",
                waits: vec![P5_HOOK],
            },
        }),
        link(
            "the runtime records the grant, admitted",
            &agent,
            "ApprovalResponded",
            Some(r#""result":"admitted""#),
            "the runtime's record of the owner's grant",
            "the core's approval flow (mandate spec §6.4)",
            vec![P5_HOOK],
        ),
        link(
            "the executor submits the opening",
            &acct,
            "OrderRequestRecorded",
            Some(r#""purpose":"open""#),
            "the opening, through the account ledger to the fixture broker",
            "the executor takes the runtime's orders (#1736)",
            vec![E2E_FILL, P5_HOOK],
        ),
        link(
            "the opening fills on the fixture",
            &acct,
            "FillApplied",
            Some(r#""side":"buy""#),
            "the paper fill of the opening on the fixture broker",
            "the executor on its fixture paper broker (DEC-2031)",
            vec![E2E_FILL],
        ),
        link(
            "the protective stop goes out",
            &acct,
            "ProtectionChanged",
            Some(r#""action":"placed""#),
            "the protective stop on the filled position, placed as the bracket's legs",
            "protective-exit sequencing in the executor (trading spec §5.4)",
            vec![E2E_FILL],
        ),
        link(
            "the news item starts a monitor run over the held thesis",
            &agent,
            "RunStarted",
            Some(r#""kind":"event""#),
            "the monitor run the news item triggers, whose trigger names the item's \
             ObservationRecorded (rule 271); the recorded item arrives 900 seconds after the \
             research agent is deployed, so a run waits for it only with \
             MANDATE_CELL_LOOP_WAIT_S",
            "the research pass offers arrived items to the core (#2575), and the recorded feed's \
             item in the runtime image's bundle (#2653)",
            vec![NE_NEWS],
        ),
        link(
            "the monitor run invalidates the thesis",
            &agent,
            "ThesisInvalidated",
            None,
            "the monitor run's verdict that ends the thesis",
            "a recording's monitor reply naming the thesis (#2632)",
            vec![NE_NEWS],
        ),
        link(
            "the thesis exit submits its sell",
            &acct,
            "OrderRequestRecorded",
            Some(r#""purpose":"discretionary_exit""#),
            "the thesis exit's sell of the position",
            "the order builder's signal exit (trading spec §8.3 step 2)",
            vec![NE_NEWS, E2E_FILL],
        ),
        link(
            "the sell fills on the fixture",
            &acct,
            "FillApplied",
            Some(r#""side":"sell""#),
            "the paper fill of the thesis exit",
            "the executor on its fixture paper broker (DEC-2031)",
            vec![NE_NEWS, E2E_FILL],
        ),
    ]
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

/// The demo's inputs: its two envelopes, the run's workspace's and the research agent's, and every
/// file the gestures' pod mounts at [`INPUTS`] under its path there (the envelopes, the instrument,
/// the research recording and the backtest runner's demo inputs).
pub struct DemoInputs {
    mandate: Vec<u8>,
    research_mandate: Vec<u8>,
    files: Vec<(String, String)>,
}

impl DemoInputs {
    /// The two envelopes [`plan`] takes.
    pub fn envelopes(&self) -> (&[u8], &[u8]) {
        (&self.mandate, &self.research_mandate)
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
    let (mandate, research_mandate) = (read(MANDATE_FILE)?, read(RESEARCH_MANDATE_FILE)?);
    let files = [
        ("mandate.json", text(mandate.clone(), MANDATE_FILE)?),
        (
            "instrument.json",
            text(read(INSTRUMENT_FILE)?, INSTRUMENT_FILE)?,
        ),
        ("rule-set.json", text(read(RULE_SET_FILE)?, RULE_SET_FILE)?),
        (
            "fee-config.json",
            text(read(FEE_CONFIG_FILE)?, FEE_CONFIG_FILE)?,
        ),
        (
            "trading-calendar.json",
            text(read(TRADING_CALENDAR_FILE)?, TRADING_CALENDAR_FILE)?,
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
    run_in(name).map(|_| ())
}

/// [`run`], answering the workspace the scenario ran in, for a step that follows it there
/// (`cargo xtask cell journey`).
///
/// # Errors
/// As [`run`].
pub fn run_in(name: &str) -> Result<String> {
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
        let checked = match connect_check(&format!("{id}c"), &workspace, &account, CONNECTION)? {
            Ok(checked) => checked,
            Err(logs) => bail!("the executor's connect check for the demo did not pass:\n{logs}"),
        };
        eprintln!("    connect check: passed as {checked} for account {account}");
        let research_workspace = research_workspace_of(&workspace);
        let research_account = research_account_of(&id);
        let research_checked = match connect_check(
            &format!("{id}r"),
            &research_workspace,
            &research_account,
            RESEARCH_CONNECTION,
        )? {
            Ok(checked) => checked,
            Err(logs) => bail!(
                "the executor's connect check for the research workspace did not pass:\n{logs}"
            ),
        };
        eprintln!(
            "    connect check: passed as {research_checked} for the research workspace's account \
             {research_account}"
        );
        Some(Connections {
            demo: Connected {
                connection: CONNECTION,
                account,
                checked,
            },
            research: Connected {
                connection: RESEARCH_CONNECTION,
                account: research_account,
                checked: research_checked,
            },
        })
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
    eprintln!("    workspace {workspace}, agent {WEB_AGENT}");
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
    let mut looped = Looped::Open;
    let mut linked = BTreeMap::<String, u64>::new();
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
                    let (word, said) = match (&failed, applied_from(browsed.as_ref())) {
                        (Some(_), _) => ("not run", "an earlier part failed".to_owned()),
                        (None, Err(why)) => ("not run", why),
                        (None, Ok(command)) => {
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
                Part::Loop(_) | Part::Grant(_) => {
                    let (shows, gap) = match part {
                        Part::Loop(check) => (check.shows, &check.gap),
                        Part::Grant(check) => (check.shows, &check.gap),
                        _ => continue,
                    };
                    let seen = match (&failed, &looped, part) {
                        (None, Looped::Open, Part::Loop(check)) => {
                            Some(journaled(check, &mut linked))
                        }
                        (None, Looped::Open, Part::Grant(check)) => Some(grant(check, &id, &files)),
                        _ => None,
                    };
                    let required = env::var_os(CELL_REQUIRED_ENV).is_some();
                    let (word, said) =
                        link_verdict(failed.is_some(), required, &mut looped, seen, gap);
                    if word == "failed" {
                        failed.get_or_insert_with(|| format!("research loop: {said}"));
                    }
                    eprintln!("    {word:<8} {shows}: {said}");
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
    Ok(workspace)
}

/// Reads `check` with the owner's session every two seconds until it passes or [`APPLIED_WITHIN`]
/// has passed: a runtime applies a kill switch on its next pass after it starts, which may be after
/// the Job ends. The bound is the clock's, not a count of reads, since each read also checks the
/// forward and signs in (about seven seconds a read on CI, so 45 reads ran past the job's ten
/// minutes; main run 40281337291). An API that cannot be asked fails it, since the cell ran.
#[expect(
    clippy::disallowed_methods,
    reason = "the host-side scenario bounds its wait for a live cell by the host's clock"
)]
fn poll_applied(check: &ApiCheck) -> Result<(&'static str, String, bool)> {
    let deadline = Instant::now() + APPLIED_WITHIN;
    loop {
        let answer = api_answer(check)?;
        let verdict = api_verdict(&answer, check, true);
        if !verdict.2 || Instant::now() >= deadline {
            return Ok(verdict);
        }
        sleep(Duration::from_secs(2));
    }
}

/// How long [`poll_applied`] waits for the kill switch to read back applied.
const APPLIED_WITHIN: Duration = Duration::from_secs(90);

/// How long the research loop waits for each of its links to appear in the journal by default: one
/// research pass at the runtime's tick, or the executor's next reconciliation, with the Job's ten
/// minutes in mind, since a link not seen stops the loop's later reads.
const LINK_WITHIN: Duration = Duration::from_secs(60);

/// The variable a rehearsal sets to wait longer for each link, in whole seconds, up to
/// [`LINK_WITHIN_MAX`]: the recorded news item arrives 900 seconds after the research agent is
/// deployed (#2653), which a CI run's wait does not reach.
pub const LOOP_WAIT_ENV: &str = "MANDATE_CELL_LOOP_WAIT_S";

/// The longest a rehearsal may wait for one link, so a mistyped value never holds a run for hours.
const LINK_WITHIN_MAX: Duration = Duration::from_secs(1_800);

/// The wait for each link: `set`, the variable's value, in whole seconds from 1 to
/// [`LINK_WITHIN_MAX`]'s, or [`LINK_WITHIN`] when it is unset or not such a number.
pub fn link_within(set: Option<&str>) -> Duration {
    set.and_then(|text| text.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|wait| !wait.is_zero() && *wait <= LINK_WITHIN_MAX)
        .unwrap_or(LINK_WITHIN)
}

/// Whether the research loop's later links are read: `Open` until a link is not seen, then
/// `Stopped` with that link, so every later link reads as not reached rather than spending its own
/// wait on what cannot follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Looped {
    Open,
    Stopped(&'static str),
}

/// What a read of a link found: the line that shows it, that it is not there, or why it could not
/// be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    There(String),
    Absent,
    Unread(String),
}

/// A link's word and what it says, given whether an earlier part failed, whether the cell is required
/// (`CELL_REQUIRED_ENV`), the loop so far, what its read found (`None` when it was not read), and the
/// gap it stands for. A link seen ran; one not seen stops the loop and reads as its gap, so a link
/// not yet on main is named, never failed; one that could not be read fails when the cell is
/// required, since the journal or the CLI broke, and otherwise reads as its gap with why; every
/// later link reads as not reached.
pub fn link_verdict(
    earlier_failed: bool,
    required: bool,
    looped: &mut Looped,
    seen: Option<Seen>,
    gap: &Gap,
) -> (&'static str, String) {
    if earlier_failed {
        return ("not run", "an earlier part failed".to_owned());
    }
    if let Looped::Stopped(at) = looped {
        return ("not reached", format!("the loop stopped at: {at}"));
    }
    let waits = || {
        gap.waits
            .iter()
            .map(|w| format!("{}: {}", w.named(), w.lands))
            .collect::<Vec<_>>()
            .join("; ")
    };
    match seen {
        Some(Seen::There(line)) => ("ran", line),
        Some(Seen::Unread(why)) if required => {
            *looped = Looped::Stopped(gap.what);
            ("failed", format!("{}, not read: {why}", gap.what))
        }
        Some(Seen::Unread(why)) => {
            *looped = Looped::Stopped(gap.what);
            (
                "gap",
                format!("{}, not read: {why} (waits on {})", gap.what, waits()),
            )
        }
        Some(Seen::Absent) | None => {
            *looped = Looped::Stopped(gap.what);
            ("gap", format!("{} (waits on {})", gap.what, waits()))
        }
    }
}

/// The text as SQL quotes it.
fn sql_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The query that finds `check`'s first event after `after` on its stream: of its type, its stored
/// body holding its fragment as a plain substring (no pattern), every literal quoted as SQL quotes
/// it; it answers that event's sequence number, or nothing.
pub fn link_query(check: &LoopCheck, after: u64) -> String {
    let holding = check
        .holding
        .map(|fragment| {
            format!(
                " AND position({} in convert_from(body, 'UTF8')) > 0",
                sql_quoted(fragment)
            )
        })
        .unwrap_or_default();
    format!(
        "SELECT seq FROM journal.events WHERE stream_id = {} AND event_type = {}{holding} AND \
         seq > {after} ORDER BY seq LIMIT 1",
        sql_quoted(&check.stream),
        sql_quoted(check.event_type)
    )
}

/// Reads `check`'s first event after the last link seen on its stream, `after`'s, from the journal's
/// primary every two seconds until one is there or [`link_within`]'s wait has passed, and records its
/// sequence number there: what Postgres holds, not what a process reports.
#[expect(
    clippy::disallowed_methods,
    reason = "the host-side scenario bounds its wait for a live cell by the host's clock"
)]
fn journaled(check: &LoopCheck, after: &mut BTreeMap<String, u64>) -> Seen {
    let since = after.get(&check.stream).copied().unwrap_or(0);
    let query = link_query(check, since);
    let wait = link_within(env::var(LOOP_WAIT_ENV).ok().as_deref());
    let deadline = Instant::now() + wait;
    loop {
        match super::on_primary(&query) {
            Ok(answer) => match answer.trim().parse::<u64>() {
                Ok(seq) => {
                    after.insert(check.stream.clone(), seq);
                    return Seen::There(format!(
                        "{} at seq {seq} of {}",
                        check.event_type, check.stream
                    ));
                }
                Err(_) if Instant::now() < deadline => sleep(Duration::from_secs(2)),
                Err(_) => return Seen::Absent,
            },
            Err(why) => return Seen::Unread(format!("{why:#}")),
        }
    }
}

/// The owner's arguments for a CLI gesture in `workspace`, as the plan's gestures carry them.
fn owner_in(workspace: &str, args: &[&str]) -> Vec<String> {
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
        .map(str::to_owned)
        .collect()
}

/// The first pending approval of `agent` that `approvals list` printed.
pub fn pending_approval(lines: &[&str], agent: &str) -> Option<String> {
    lines.iter().find_map(|line| {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            [approval, of, "pending", ..] if *of == agent => Some((*approval).to_owned()),
            _ => None,
        }
    })
}

/// Whether the content `approvals show` printed asks an opening, the one approval the research loop
/// grants (mandate spec §6.4: only `open` and `increase` are ever asked).
pub fn shows_opening(lines: &[&str]) -> bool {
    lines
        .iter()
        .any(|line| line.starts_with('{') && line.contains(r#""purpose":"open""#))
}

/// What `approvals approve` printed: its `granted` line when the line after it says the grant was
/// admitted or is not recorded yet (`approvals::message`), the runtime's record being the next
/// link; else why not, a refusal, a count short of the approvers, or a skip on re-validation, each a
/// grant that did not do what it shows.
pub fn granted(lines: &[&str], approval: &str) -> Seen {
    let head = format!("granted {approval} as event ");
    let Some(at) = lines.iter().position(|l| l.starts_with(&head)) else {
        return Seen::Unread(format!("`approvals approve {approval}` granted nothing"));
    };
    match lines.get(at + 1) {
        Some(said)
            if said.starts_with("Admitted and sent")
                || said.starts_with("Admitted, not yet re-validated")
                || said.starts_with("Not recorded yet") =>
        {
            Seen::There(format!("{}; {said}", lines[at]))
        }
        Some(said) => Seen::Unread(format!("{}; {said}", lines[at])),
        None => Seen::Unread(format!("{}; no outcome printed", lines[at])),
    }
}

/// The code `approvals show` printed.
pub fn shown_code(lines: &[&str]) -> Option<String> {
    lines
        .iter()
        .find_map(|line| line.strip_prefix("code "))
        .map(str::to_owned)
}

/// Runs one CLI gesture as its own Job after the gestures' Job, as `confirm_on_host` does, and
/// returns its lines when it exited cleanly.
fn control_gesture(
    gesture: &Gesture,
    run: &str,
    files: &[(&str, String)],
) -> std::result::Result<Vec<String>, String> {
    let ran = || -> Result<(bool, String)> {
        let rendered = render(
            &[
                format!("run.id=\"{run}\""),
                format!("run.args={}", json!(gesture.args)),
            ],
            Some("templates/run.yaml"),
        )?;
        run_job(gestures_job(
            rendered,
            &[gesture],
            &format!("demo-in-{run}"),
            files,
            NAMESPACE,
        )?)
    };
    match ran() {
        Ok((true, logs)) => Ok(lines_of(&logs, &gesture.container)
            .iter()
            .map(|l| (*l).to_owned())
            .collect()),
        Ok((false, logs)) => Err(format!(
            "`mandate {}` failed:\n{logs}",
            gesture.args.join(" ")
        )),
        Err(why) => Err(format!("{why:#}")),
    }
}

/// The owner's grant of `check`'s agent's pending opening: `approvals list` finds it, `approvals
/// show` shows its code, and `approvals approve` types that code, each its own Job in the research
/// workspace. A list with no pending approval is the link not seen.
fn grant(check: &GrantCheck, id: &str, files: &[(&str, String)]) -> Seen {
    let step = |n: u8, container: &str, args: &[&str], expect: &str| Gesture {
        container: format!("g9{n}-{container}"),
        shows: check.shows,
        command: None,
        args: owner_in(&check.workspace, args),
        expect: expect.to_owned(),
    };
    let listed = match control_gesture(
        &step(
            5,
            "approvals-list",
            &["approvals", "list", "--agent", check.agent],
            "pending",
        ),
        &format!("{id}l"),
        files,
    ) {
        Ok(lines) => lines,
        Err(why) => return Seen::Unread(why),
    };
    let lines: Vec<&str> = listed.iter().map(String::as_str).collect();
    let Some(approval) = pending_approval(&lines, check.agent) else {
        return Seen::Absent;
    };
    let shown = match control_gesture(
        &step(
            6,
            "approvals-show",
            &["approvals", "show", &approval, "--agent", check.agent],
            "code ",
        ),
        &format!("{id}w"),
        files,
    ) {
        Ok(lines) => lines,
        Err(why) => return Seen::Unread(why),
    };
    let lines: Vec<&str> = shown.iter().map(String::as_str).collect();
    if !shows_opening(&lines) {
        return Seen::Unread(format!(
            "`approvals show {approval}` shows no opening, so it is not granted"
        ));
    }
    let Some(code) = shown_code(&lines) else {
        return Seen::Unread(format!("`approvals show {approval}` showed no code"));
    };
    match control_gesture(
        &step(
            7,
            "approvals-approve",
            &[
                "approvals",
                "approve",
                &approval,
                "--agent",
                check.agent,
                "--code",
                &code,
            ],
            "granted ",
        ),
        &format!("{id}g"),
        files,
    ) {
        Ok(lines) => granted(
            &lines.iter().map(String::as_str).collect::<Vec<_>>(),
            &approval,
        ),
        Err(why) => Seen::Unread(why),
    }
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

/// The Deployments [`point_api`] points at the run and waits on: the API, the first manager and
/// the first scheduler on the run's workspace, and the second manager, the second scheduler and the
/// model gateway on its research workspace (DEC-2070, DEC-2071).
const POINTED: [&str; 6] = [
    API_PROCESS,
    DEPLOY_MANAGER_PROCESS,
    SCHEDULER_PROCESS,
    RESEARCH_MANAGER_PROCESS,
    RESEARCH_SCHEDULER_PROCESS,
    MODEL_GATEWAY_PROCESS,
];

/// The chart values [`point_api`] sets for the run's `workspace`: the API on it, and the second
/// manager and the model gateway on its research workspace, the same ULID with `r`, the one value
/// the chart takes for either (DEC-2070 items 1 and 5).
fn pointed_values(workspace: &str) -> [String; 3] {
    let research = research_workspace_of(workspace);
    [
        format!("api.workspace={workspace}"),
        format!("deployManager.researchWorkspace={research}"),
        format!("modelGateway.workspace={research}"),
    ]
}

/// Points the cell's API and deployment manager at the run's `workspace` (DEC-2010), and the second
/// manager and the model gateway at its research workspace (DEC-2070, DEC-2397 item 6): a `helm
/// upgrade` of the workspace release that keeps every other value, then a wait until each of those
/// Deployments the cell holds has rolled out, so no pod of an earlier run still serves another
/// workspace; and
/// any earlier `kubectl port-forward` to the API is stopped, so the checks never reach an old pod
/// through it (`api_port_forward` would reuse a forward that still answers its 401).
pub(super) fn point_api(workspace: &str) -> Result<()> {
    let mut args = vec![
        "upgrade".to_owned(),
        "workspace".to_owned(),
        WORKSPACE_CHART.to_owned(),
        "--namespace".to_owned(),
        NAMESPACE.to_owned(),
        "--reuse-values".to_owned(),
    ];
    for value in pointed_values(workspace) {
        args.extend(["--set".to_owned(), value]);
    }
    args.extend(["--wait", "--wait-for-jobs", "--timeout", "3m"].map(str::to_owned));
    let upgraded = helm(&args.iter().map(String::as_str).collect::<Vec<_>>());
    if upgraded.is_err() {
        api_diagnosis();
    }
    upgraded?;
    for process in POINTED {
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

/// The scenario's one pass through the browser on `agent`, the run's workspace's one agent, which
/// nothing in the gestures' Job stops: first the trade loop (lane W's spec, its confirmation run on the host as a control
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
/// stream and reads it accepted. It does not require `applied`: step 7's [`AppliedCheck`] then
/// reads the same command back until the agent's runtime applied it (DEC-2379).
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

/// The command step 7's applied check reads back: the one web/'s kill switch recorded, or why
/// there is none. A browser that could not run, or ran after an earlier failure, leaves nothing
/// to read, which reads `not run` as web/'s own parts do (their verdict fails it where web/ is
/// required), never as a failure of its own.
pub fn applied_from(browsed: Option<&WebOutcome>) -> Result<String, String> {
    match browsed {
        Some(WebOutcome::Recorded { command, .. }) => Ok(command.clone()),
        Some(WebOutcome::Unaskable(why) | WebOutcome::NoTools(why)) => {
            Err(format!("web/'s kill switch did not run: {why}"))
        }
        Some(WebOutcome::Failed(_) | WebOutcome::AfterFailure) | None => {
            Err("an earlier part failed".to_owned())
        }
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

    fn research_mandate() -> Vec<u8> {
        fs::read(format!("../{RESEARCH_MANDATE_FILE}")).unwrap()
    }

    /// The connect check a run's plan names: run 1's account, and a passing check's event id.
    fn connected() -> Connected {
        Connected {
            connection: CONNECTION,
            account: account_of("1"),
            checked: "01KDEC0000000000000000000C".into(),
        }
    }

    /// The research workspace's connect check a run's plan names: run 1's research account, and
    /// a passing check's event id.
    fn research_connected() -> Connected {
        Connected {
            connection: RESEARCH_CONNECTION,
            account: research_account_of("1"),
            checked: "01KDEC0000000000000000000R".into(),
        }
    }

    /// The plan for `workspace` on the demo's three envelopes, after [`connected`]'s and
    /// [`research_connected`]'s checks.
    fn planned(workspace: &str) -> Vec<Step> {
        plan(
            workspace,
            (&mandate(), &research_mandate()),
            Some(&Connections {
                demo: connected(),
                research: research_connected(),
            }),
        )
    }

    /// The plan the staging dry run renders, which names no connect check.
    fn unconnected(workspace: &str) -> Vec<Step> {
        plan(workspace, (&mandate(), &research_mandate()), None)
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
            RULE_SET_FILE,
            FEE_CONFIG_FILE,
            TRADING_CALENDAR_FILE,
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

    /// The QQQ envelope, a test fixture now, is the demo's but for its name, its description and its one
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
                CONNECTION,
                "01J0ACC0VNT000000000000000",
                "10000000000000000000000001",
                "user-owner"
            ),
            "e92fb7e5",
            "the code `connection record` showed for an Alpaca paper record of the demo's \
             connection, scope trading, on that account, caused by that check, for that owner"
        );
        let version = "sha256:8e09904f299adb4afc9e09343071e36953db6b7faf5dc52494a86fe9a1dcb89f";
        assert_eq!(confirm_code(version), "ab1b80cd");
        assert_eq!(deploy_code(WEB_AGENT, version), "9056947e");
        assert_eq!(code(""), "e3b0c442");
        let research = reference(&research_mandate());
        assert_eq!(
            research,
            "sha256:f63145eb604ff161807d849c23a24e53fbd2e535a0e0db52ff775a9de6b533ce"
        );
        assert_eq!(confirm_code(&research), "30eb4e5e");
        assert_eq!(deploy_code(RESEARCH_AGENT, &research), "9400be68");
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

    /// The journey accepts the version `mandate disclosure accept` stores, which its own test pins,
    /// with the code the CLI shows for it.
    #[test]
    fn the_journey_accepts_the_go_live_disclosures_pinned_version_with_its_code() -> Result<()> {
        let version = disclosure_version()?;
        assert_eq!(
            version,
            "sha256:35ab1cb9c3861954a3d846866198e8b4c801532de56e8b00ee86e51404d541f2"
        );
        let gesture = format!(
            r#"{{"document":"go_live_risk","gesture":"disclosure_accept","version":"{version}"}}"#
        );
        assert_eq!(disclosure_code(&version), code(&gesture));
        assert_eq!(disclosure_code(&version).len(), 8);
        Ok(())
    }

    /// Step 5 runs the runner on the confirmed version in the pod's store and records the payload
    /// it prints, after the confirmation and before the kill switch, under the rule set step 2
    /// registered before the version; the API read and web/'s report wait on the membership
    /// parser and web/.
    #[test]
    fn step_five_runs_the_backtest_under_the_registered_rule_set_and_records_it() {
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
        let (confirm, export) = (
            all.iter()
                .rposition(|g| g.args[..3] == ["version", "confirm", version.as_str()])
                .unwrap(),
            at("journal-export"),
        );
        assert!(rule_set < confirm && confirm < runner && runner < record && record < export);
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

    /// Step 2 registers the reviewed configuration before any version, once each in each
    /// workspace, the run's and its research workspace's (DEC-2819 item 3): the gate's rule set,
    /// which the runtime's gate and the executor's binding gate both read as the control stream's
    /// latest `rule_set`, so no later registration of that kind may shadow it, then the paper fee
    /// schedule, its trading calendar and SPY's instrument snapshot. Each file is the shell's
    /// reviewed tracer configuration, and the rule set has exactly the members the gates accept.
    #[test]
    fn step_two_registers_the_reviewed_configuration_both_gates_read_once_before_any_version() {
        let steps = planned("demo2");
        let every = gestures(&steps);
        for workspace in ["demo2", "demo2r"] {
            let all: Vec<&&Gesture> = every
                .iter()
                .filter(|g| g.args.windows(2).any(|w| w == ["--workspace", workspace]))
                .collect();
            let first_version = all
                .iter()
                .position(|g| g.args[..2] == ["version", "create"])
                .unwrap();
            for (kind, file) in [
                ("rule_set", "rule-set.json"),
                ("fee_config", "fee-config.json"),
                ("trading_calendar", "trading-calendar.json"),
                ("instrument_snapshot", "instrument.json"),
            ] {
                let registered: Vec<usize> = all
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| g.args.windows(2).any(|w| w == ["--kind", kind]))
                    .map(|(n, _)| n)
                    .collect();
                assert_eq!(
                    registered.len(),
                    1,
                    "{workspace}: {kind} is registered once"
                );
                assert!(
                    registered[0] < first_version,
                    "{workspace}: {kind} before any version"
                );
                let path = format!("{INPUTS}/{file}");
                assert!(
                    all[registered[0]].args.contains(&path),
                    "{workspace}: {kind} registers {path}: {:?}",
                    all[registered[0]].args
                );
            }
        }
        for (demo, tracer) in [
            (RULE_SET_FILE, "rule-set.json"),
            (FEE_CONFIG_FILE, "fee-config.json"),
            (TRADING_CALENDAR_FILE, "trading-calendar.json"),
        ] {
            assert_eq!(
                parsed(demo),
                parsed(&format!(
                    "crates/mandate-shell/tests/fixtures/tracer/config/{tracer}"
                )),
                "{demo} is the shell's reviewed configuration"
            );
        }
        let rule_set = parsed(RULE_SET_FILE);
        let members: Vec<&str> = rule_set
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            members,
            [
                "executor",
                "gate",
                "gate_config",
                "iex_quote_max_age_s",
                "ruleset_version",
                "story"
            ],
            "mandate-gate-inputs' RULE_MEMBERS"
        );
        assert_eq!(rule_set["gate"], "trading-domain-9.1");
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
        assert_eq!(m["connection_id"], RESEARCH_CONNECTION);
        assert_ne!(
            m["connection_id"],
            parsed(MANDATE_FILE)["connection_id"],
            "a connection of its own (DEC-2070 item 2)"
        );
    }

    /// Step 3 opens and founds the research agent's own workspace, records there the paper
    /// connection of its own connect check, registers its two models, the research one from the
    /// recorded entry, then creates, confirms and deploys its envelope, every gesture in that
    /// workspace and none in the run's, so the research agent is alone on its control stream and
    /// its deployment is written at version 2 (DEC-2214 item 1, DEC-2247 items 2 and 3). The
    /// executor the manager starts for that account is checked after the Job; what is left is a
    /// check of the research run itself.
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
                ["connection", "record"],
                ["connection", "record"],
                ["config", "register"],
                ["config", "register"],
                ["config", "register"],
                ["config", "register"],
                ["model", "register"],
                ["model", "register"],
                ["version", "create"],
                ["version", "confirm"],
                ["version", "confirm"],
                ["agent", "deploy"],
                ["agent", "deploy"],
            ]
        );
        let research = research_connected();
        let code = record_code(
            RESEARCH_CONNECTION,
            &research.account,
            &research.checked,
            &FOUNDER.to_ascii_lowercase(),
        );
        assert_eq!(gestures[2].expect, format!("code {code}"));
        assert!(
            !gestures[2].args.iter().any(|a| a == "--code"),
            "the shown record commits nothing"
        );
        for g in &gestures[2..4] {
            assert!(
                g.args
                    .windows(2)
                    .any(|w| w == ["--connection", RESEARCH_CONNECTION]),
                "{:?}",
                g.args
            );
        }
        assert!(research.account.starts_with("01KDER"));
        assert_ne!(
            research.account,
            connected().account,
            "an account of its own"
        );
        assert_ne!(research_account_of("7"), account_of("7"));
        assert!(
            gestures[3]
                .args
                .windows(2)
                .any(|w| w == ["--code", code.as_str()])
        );
        for pair in [
            ["--account-ref", research.account.as_str()],
            ["--checked", research.checked.as_str()],
        ] {
            assert!(gestures[3].args.windows(2).any(|w| w == pair), "{pair:?}");
        }
        let executor = step
            .parts
            .iter()
            .position(|p| matches!(p, Part::Executor(e) if e.account == research.account))
            .expect("the research workspace's executor is checked");
        let acct = format!("acct:demo3r:{}", research.account);
        let agent = format!("agent:demo3r:{RESEARCH_AGENT}");
        let links: Vec<(String, &str, Option<&str>)> = step.parts[executor + 1..]
            .iter()
            .map(|p| match p {
                Part::Loop(l) => (l.stream.clone(), l.event_type, l.holding),
                Part::Grant(g) => {
                    assert_eq!((g.workspace.as_str(), g.agent), ("demo3r", RESEARCH_AGENT));
                    ("ctl:demo3r".to_owned(), "grant", None)
                }
                other => panic!("after the executor, only the loop's links: {other:?}"),
            })
            .collect();
        assert_eq!(
            links,
            [
                (agent.clone(), "RunStarted", None),
                (agent.clone(), "ThesisProposed", Some(r#""admitted":true"#)),
                (
                    agent.clone(),
                    "ApprovalRequested",
                    Some(r#""purpose":"open""#)
                ),
                ("ctl:demo3r".to_owned(), "grant", None),
                (
                    agent.clone(),
                    "ApprovalResponded",
                    Some(r#""result":"admitted""#)
                ),
                (
                    acct.clone(),
                    "OrderRequestRecorded",
                    Some(r#""purpose":"open""#)
                ),
                (acct.clone(), "FillApplied", Some(r#""side":"buy""#)),
                (
                    acct.clone(),
                    "ProtectionChanged",
                    Some(r#""action":"placed""#)
                ),
                (agent.clone(), "RunStarted", Some(r#""kind":"event""#)),
                (agent.clone(), "ThesisInvalidated", None),
                (
                    acct.clone(),
                    "OrderRequestRecorded",
                    Some(r#""purpose":"discretionary_exit""#)
                ),
                (acct, "FillApplied", Some(r#""side":"sell""#)),
            ],
            "each link of the research loop, in the story's order, after the executor is live"
        );
        for g in &gestures {
            let at = g.args.iter().position(|a| a == "--workspace").unwrap();
            assert_eq!(g.args[at + 1], "demo3r", "{:?}", g.args);
        }
        assert_eq!(
            gestures[9].args[2..6],
            [
                "llm.research_agent",
                "0.1.0",
                "--entry",
                "/in/research.json"
            ]
        );
        let version = reference(&research_mandate());
        assert_eq!(gestures[10].expect, format!("created {version}"));
        let deployed = &gestures[14].args;
        assert_eq!(deployed[2..4], [RESEARCH_AGENT, version.as_str()]);
        assert_eq!(
            lanes(step),
            ["GW", "D1", "E2E", "NE"],
            "the loop's links wait on the research run, the ask, the fill and the news"
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

    /// Step 6 runs no gesture in the Job: an owner's exit before the trade loop's fill sells
    /// nothing, so it is a gap until web/'s exit runs after the fill, in the browser.
    #[test]
    fn step_six_waits_on_web_s_exit_after_the_fill() {
        let steps = planned("demo1");
        let step6 = &steps[5];
        assert_eq!(step6.number, 6);
        assert!(
            step6.parts.iter().all(|p| matches!(p, Part::Gap(_))),
            "no gesture of the Job exits"
        );
        assert!(
            gestures(&steps)
                .iter()
                .all(|g| g.args[..2] != ["agent", "exit"])
        );
        assert_eq!(lanes(step6), ["WB"]);
        let pinned = parsed(INSTRUMENT_FILE);
        assert_eq!(pinned["instrument_id"], SPY, "SPY's Alpaca asset id");
        assert_eq!(pinned["symbol"], "SPY");
    }

    /// The run's workspace holds one agent, [`WEB_AGENT`], on the demo's envelope (V-048, DEC-802
    /// item 3): its one version is created, confirmed and deployed in step 2, its code shown and
    /// then typed, and no gesture of the Job stops it, so the browser finds it running.
    #[test]
    fn the_run_s_workspace_holds_one_agent_the_job_never_stops() {
        let version = reference(&mandate());
        let steps = planned("demo6");
        let all = gestures(&steps);
        let in_run = |g: &&&Gesture| g.args.iter().any(|a| a == "demo6");
        let deployed: Vec<[&str; 2]> = all
            .iter()
            .filter(in_run)
            .filter(|g| g.args[..2] == ["agent", "deploy"])
            .map(|g| [g.args[2].as_str(), g.args[3].as_str()])
            .collect();
        let v = version.as_str();
        assert_eq!(deployed, [[WEB_AGENT, v], [WEB_AGENT, v]]);
        let created: Vec<&&Gesture> = all
            .iter()
            .filter(in_run)
            .filter(|g| g.args[..2] == ["version", "create"])
            .collect();
        assert_eq!(created.len(), 1, "one envelope in the run's workspace");
        assert!(
            all.iter().all(|g| g.args[..2] != ["agent", "kill"]),
            "no gesture of the Job stops the agent"
        );
        assert!(
            all.iter()
                .filter(in_run)
                .all(|g| !g.args.iter().any(|a| a.contains("web-mandate"))),
            "no second envelope"
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
                (1, read.as_str(), 200, Ask::Session, &Body::Lists(WEB_AGENT)),
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
        let exit = all
            .iter()
            .find(|g| g.args[..2] == ["agent", "deploy"])
            .unwrap();
        let user = owner.to_ascii_lowercase();
        assert!(
            exit.args
                .windows(2)
                .any(|w| w[0] == "--user" && w[1] == user),
            "the owner's gestures act as that member: {:?}",
            exit.args
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
    /// `main` make steps 2 to 5 partial, step 3's research gestures among them; step 6 is skipped
    /// until web/'s exit runs after the fill; the kill switch,
    /// applied by the runtime step 2's deployment starts, and the record run whole. Without a
    /// connect check (the staging dry run) step 7 is partial.
    #[test]
    fn each_step_reads_ready_partial_or_skipped_from_its_parts() {
        let steps = planned("demo1");
        let read: Vec<Readiness> = steps.iter().map(readiness).collect();
        assert_eq!(
            read,
            [
                Ready, Partial, Partial, Partial, Partial, Skipped, Ready, Ready
            ]
        );
        let dry: Vec<Readiness> = unconnected("demo1").iter().map(readiness).collect();
        assert_eq!(
            dry,
            [
                Ready, Partial, Partial, Partial, Partial, Skipped, Partial, Ready
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
    const LANDED_SLICES: &[(&str, u32)] = &[("X1a", 1596), ("X1b", 1736), ("X1b-3", 2624)];

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
            .position(|g| {
                g.args[..2] == ["connection", "record"] && g.args.iter().any(|a| a == "demo1")
            })
            .unwrap();
        let typed = all
            .iter()
            .position(|g| {
                g.args[..2] == ["connection", "record"]
                    && g.args.iter().any(|a| a == "demo1")
                    && g.args.iter().any(|a| a == "--code")
            })
            .unwrap();
        let code = record_code(CONNECTION, &c.account, &c.checked, &user);
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
        let deployed = at(&["agent", "deploy", WEB_AGENT], "--code");
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
        assert_eq!(
            executors
                .iter()
                .map(|e| e.account.as_str())
                .collect::<Vec<_>>(),
            [c.account.as_str(), research_connected().account.as_str()],
            "the demo's executor, then the research workspace's"
        );
        assert!(matches!(step2.parts.last(), Some(Part::Executor(_))));
        let dry = unconnected("demo1");
        let dry_all = gestures(&dry);
        assert!(dry_all.iter().all(|g| g.args[0] != "connection"));
        assert!(
            dry[2].parts.iter().any(|p| matches!(
                p,
                Part::Gap(gap) if gap.waits.iter().any(|w| w.lands == DR_CONNECT.lands)
            )),
            "step 3 names the research connection's gap"
        );
        assert!(
            dry.iter()
                .flat_map(|s| &s.parts)
                .all(|p| !matches!(p, Part::Executor(_) | Part::Applied(_)))
        );
    }

    /// A link seen ran; the first not seen reads as its gap, naming its lanes, and stops the loop,
    /// so every later link reads as not reached; a link that could not be read is a gap too, with
    /// why; and after an earlier part failed, no link is run.
    #[test]
    fn a_link_not_seen_names_its_gap_and_stops_the_loop() {
        let gap = Gap {
            what: "the fill",
            on_main: "x",
            waits: vec![E2E_FILL, NE_NEWS],
        };
        let mut looped = Looped::Open;
        assert_eq!(
            link_verdict(
                false,
                false,
                &mut looped,
                Some(Seen::There("1 FillApplied".into())),
                &gap
            ),
            ("ran", "1 FillApplied".to_owned())
        );
        assert_eq!(looped, Looped::Open);
        let (word, said) = link_verdict(false, false, &mut looped, Some(Seen::Absent), &gap);
        assert_eq!(word, "gap");
        assert!(
            said.starts_with("the fill (waits on E2E (E21-54): "),
            "{said}"
        );
        assert!(said.contains("NE (E19-12): "), "{said}");
        assert_eq!(looped, Looped::Stopped("the fill"));
        assert_eq!(
            link_verdict(false, false, &mut looped, None, &gap),
            ("not reached", "the loop stopped at: the fill".to_owned())
        );
        let mut open = Looped::Open;
        let (word, said) = link_verdict(
            false,
            false,
            &mut open,
            Some(Seen::Unread("no pod".into())),
            &gap,
        );
        assert_eq!(word, "gap");
        assert!(
            said.starts_with("the fill, not read: no pod (waits on "),
            "{said}"
        );
        assert_eq!(open, Looped::Stopped("the fill"));
        let mut after = Looped::Open;
        assert_eq!(
            link_verdict(true, false, &mut after, Some(Seen::There("x".into())), &gap),
            ("not run", "an earlier part failed".to_owned())
        );
        assert_eq!(after, Looped::Open);
        let mut required = Looped::Open;
        assert_eq!(
            link_verdict(
                false,
                true,
                &mut required,
                Some(Seen::Unread("no pod".into())),
                &gap
            ),
            ("failed", "the fill, not read: no pod".to_owned())
        );
        assert_eq!(required, Looped::Stopped("the fill"));
        let mut absent = Looped::Open;
        assert_eq!(
            link_verdict(false, true, &mut absent, Some(Seen::Absent), &gap).0,
            "gap",
            "a link not seen is its gap even when the cell is required"
        );
        let mut both = Looped::Stopped("the fill");
        assert_eq!(
            link_verdict(true, false, &mut both, None, &gap).0,
            "not run",
            "an earlier failure reads first"
        );
    }

    /// A link's query counts its type on its stream, its body holding the fragment when it names
    /// one, every literal quoted as SQL quotes it.
    #[test]
    fn a_link_reads_its_type_on_its_stream_from_the_journal() {
        let check = |holding| LoopCheck {
            shows: "x",
            stream: "acct:demo1r:01KDER0000000001000000000A".into(),
            event_type: "FillApplied",
            holding,
            gap: Gap {
                what: "x",
                on_main: "x",
                waits: vec![],
            },
        };
        assert_eq!(
            link_query(&check(None), 0),
            "SELECT seq FROM journal.events WHERE stream_id = \
             'acct:demo1r:01KDER0000000001000000000A' AND event_type = 'FillApplied' AND seq > 0 \
             ORDER BY seq LIMIT 1"
        );
        assert_eq!(
            link_query(&check(Some(r#""side":"buy""#)), 7),
            "SELECT seq FROM journal.events WHERE stream_id = \
             'acct:demo1r:01KDER0000000001000000000A' AND event_type = 'FillApplied' AND \
             position('\"side\":\"buy\"' in convert_from(body, 'UTF8')) > 0 AND seq > 7 ORDER BY \
             seq LIMIT 1"
        );
        assert!(link_query(&check(Some("o'clock")), 0).contains("position('o''clock' in"));
    }

    /// A rehearsal waits as long as it sets, from a second to half an hour; anything else waits the
    /// default, so a CI run's wait never grows by accident.
    #[test]
    fn a_rehearsal_sets_how_long_each_link_is_waited_for() {
        assert_eq!(link_within(None), LINK_WITHIN);
        assert_eq!(link_within(Some("1200")), Duration::from_secs(1_200));
        assert_eq!(link_within(Some(" 1 ")), Duration::from_secs(1));
        assert_eq!(link_within(Some("1800")), LINK_WITHIN_MAX);
        for refused in ["0", "1801", "-5", "20m", ""] {
            assert_eq!(link_within(Some(refused)), LINK_WITHIN, "{refused}");
        }
    }

    /// The grant reads the CLI's own lines (`mandate_cli::inbox`): the first of the agent's
    /// approvals that is pending, and the code `approvals show` printed.
    #[test]
    fn the_grant_reads_the_pending_approval_and_its_code() {
        let listed = [
            "01KAPPROVED00000000000000A demo-research acted deadline 600 remaining 0",
            "01KOTHER0000000000000000AA demo-spy pending deadline 600 remaining 500",
            "01KPENDING00000000000000AA demo-research pending deadline 600 remaining 540",
        ];
        assert_eq!(
            pending_approval(&listed, RESEARCH_AGENT).as_deref(),
            Some("01KPENDING00000000000000AA")
        );
        assert_eq!(pending_approval(&["no approvals"], RESEARCH_AGENT), None);
        let shown = [
            "approval 01KPENDING00000000000000AA deadline 540",
            "{}",
            "content_hash sha256:00",
            "code 1a2b3c4d",
        ];
        assert_eq!(shown_code(&shown).as_deref(), Some("1a2b3c4d"));
        assert_eq!(shown_code(&shown[..3]), None);
        assert!(!shows_opening(&shown), "an empty content asks nothing");
        assert!(shows_opening(&[r#"{"purpose":"open","side":"buy"}"#]));
        assert!(!shows_opening(&[r#"{"purpose":"increase","side":"buy"}"#]));
        let id = "01KPENDING00000000000000AA";
        let line = format!("granted {id} as event 01KEV at seq 9 for sha256:00");
        for (said, seen) in [
            (
                "Admitted and sent to the executor; the gate still decides.",
                true,
            ),
            ("Admitted, not yet re-validated; nothing was sent.", true),
            ("Not recorded yet. If the runtime does not record it", true),
            ("Refused (step_up_stale); nothing was sent.", false),
            (
                "Counted, short of the approvers required; nothing was sent.",
                false,
            ),
            (
                "Admitted, then skipped on re-validation (x); nothing was sent.",
                false,
            ),
        ] {
            let got = granted(&[line.as_str(), said], id);
            assert_eq!(matches!(got, Seen::There(_)), seen, "{said}: {got:?}");
            if !seen {
                assert!(
                    matches!(got, Seen::Unread(ref why) if why.contains(said)),
                    "{got:?}"
                );
            }
        }
        assert!(matches!(granted(&[line.as_str()], id), Seen::Unread(_)));
        assert!(matches!(
            granted(&["granted 01KOTHER as event x"], id),
            Seen::Unread(_)
        ));
    }

    /// The grant's gestures carry the owner's arguments exactly as the plan's research gestures do.
    #[test]
    fn the_grant_acts_as_the_owner_in_the_research_workspace() {
        let steps = planned("demo3");
        let registered = gestures_of(&steps, |n| n == 3)
            .into_iter()
            .find(|g| g.args[..2] == ["model", "register"])
            .expect("a research gesture");
        let at = registered
            .args
            .iter()
            .position(|a| a == "--workspace")
            .unwrap();
        assert_eq!(
            owner_in("demo3r", &["approvals", "list"])[2..],
            registered.args[at..],
            "the same workspace, user, journal and store"
        );
    }

    /// Each link's event and the member its fragment names are the journal's own: the catalogue row
    /// of the event (journal spec §9) names the member, or, for `FillApplied`'s side, which the row
    /// leaves to §9.5, the executor's writer journals it; read from those files, not from the plan.
    #[test]
    fn each_link_reads_a_member_its_event_carries() {
        let spec = fs::read_to_string("../docs/specs/journal.md").unwrap();
        let writer = fs::read_to_string("../crates/mandate-executor/src/orders.rs").unwrap();
        let steps = planned("demo3");
        let checks: Vec<&LoopCheck> = steps[2]
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Loop(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(checks.len(), 11);
        for check in checks {
            let named = format!("`{}`", check.event_type);
            let row = spec
                .lines()
                .find(|l| {
                    l.starts_with("| `") && l.split('|').nth(1).is_some_and(|n| n.contains(&named))
                })
                .unwrap_or_else(|| panic!("{} has a catalogue row", check.event_type));
            let Some(fragment) = check.holding else {
                continue;
            };
            let member = fragment.split('"').nth(1).unwrap();
            let written =
                check.event_type == "FillApplied" && writer.contains(&format!("(\"{member}\", "));
            assert!(
                row.contains(member) || written,
                "{}: {member}",
                check.event_type
            );
        }
    }

    /// DEC-2070 items 1 and 5: the cell is pointed at the run's workspace and, for the second
    /// manager and the model gateway alike, at its research workspace, the run's with `r`, which
    /// the chart's schema and guard take; and each of those Deployments is waited on.
    #[test]
    fn the_cell_is_pointed_at_the_run_and_its_research_workspace() {
        let run = workspace_of("12");
        assert_eq!(
            pointed_values(&run),
            [
                format!("api.workspace={run}"),
                format!("deployManager.researchWorkspace={run}r"),
                format!("modelGateway.workspace={run}r"),
            ]
        );
        assert_eq!(
            POINTED,
            [
                "api",
                "deploy-manager",
                "scheduler",
                "deploy-manager-research",
                "scheduler-research",
                "model-gateway"
            ]
        );
    }

    /// Step 7's applied check reads the command web/'s kill switch recorded, and reads anything
    /// else as `not run`, never as a failure of its own: a machine without the browser's tools, or
    /// with the API out of reach, still finishes, as web/'s own parts do.
    #[test]
    fn the_applied_check_reads_the_recorded_command_or_not_run() {
        let recorded = WebOutcome::Recorded {
            agent: WEB_AGENT.into(),
            workspace: "demo1".into(),
            command: "1TSG".into(),
            read: Ok((200, String::new())),
        };
        assert_eq!(applied_from(Some(&recorded)), Ok("1TSG".into()));
        for outcome in [
            WebOutcome::NoTools("no npm".into()),
            WebOutcome::Unaskable("no API".into()),
            WebOutcome::Failed("timed out".into()),
            WebOutcome::AfterFailure,
        ] {
            assert!(applied_from(Some(&outcome)).is_err(), "{outcome:?}");
        }
        assert!(applied_from(None).is_err());
        assert!(
            applied_from(Some(&WebOutcome::NoTools("no npm".into())))
                .is_err_and(|why| why.contains("no npm")),
            "names why"
        );
    }

    /// DEC-2379: step 7 is web/'s kill switch on the one agent, followed by the check that its
    /// runtime applied it, which reads the command the browser recorded and passes on `applied`
    /// alone. Nothing in the gestures' Job stops the agent first: no `agent kill` is a gesture.
    #[test]
    fn step_seven_reads_web_s_kill_switch_back_applied() {
        let steps = planned("demo1");
        let step7 = &steps[6].parts;
        assert!(matches!(&step7[0], Part::Web(c) if c.agent == WEB_AGENT));
        assert!(
            matches!(&step7[1], Part::Applied(c) if c.agent == WEB_AGENT),
            "the runtime of the agent web/ killed"
        );
        assert!(
            gestures(&steps)
                .iter()
                .all(|g| g.args[..2] != ["agent", "kill"]),
            "no CLI kill switch stops the agent before the browser"
        );
        let read = applied_read("demo1", WEB_AGENT, "1TSG");
        assert_eq!(read.path, "/v1/workspaces/demo1/commands/1TSG");
        let with_steps = |phase: &str, steps: Value| {
            json!({"command_id": "1TSG", "phase": phase, "steps": steps,
                "as_of": [{"stream_id": "ctl:demo1"}]})
            .to_string()
        };
        let runtime = json!([{"stream_id": "agent:demo1:agt_01KDEM00000000000000000WEB",
            "event_type": "KillSwitchActivated"}]);
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
            json!([{"stream_id": "agent:demo1:demo-spy", "event_type": "KillSwitchActivated"}]),
            json!([{"stream_id": "agent:demo1:agt_01KDEM00000000000000000WEB", "event_type": "AgentModeApplied"}]),
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

    /// Step 4 runs lane W's trade loop on the run's workspace's one agent, which nothing stops
    /// before the parts run, and reads it: `ran` only when every step
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
                if let Some(gap) = part.gap() {
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
