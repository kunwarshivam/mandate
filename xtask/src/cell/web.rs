//! How `cargo xtask cell demo` runs `web/` (lane W2, DEC-2406): on its own fixtures, or against the
//! cell's API as the demo owner once sign-in through the stub issuer is ready. The scenario's web
//! gestures (`scenario.rs`, lane E2E) call [`kill_switch_from_web`], which starts its own `web/`
//! and drives it with Playwright (`web/e2e-cell/`).

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use super::{DEMO_APP_ORIGIN, DEMO_SUBJECT, LOCAL_ISSUER, WEB_URL, state};

/// The local API's port-forward on `localhost`, as `cargo xtask cell demo`'s own `web/` reaches it.
const WEB_API_ORIGIN: &str = "http://localhost:8787";

/// The API as the cell gesture's page reaches it: the https front `web/e2e-cell/tls-proxy.mjs`
/// serves in front of [`WEB_API_ORIGIN`]. Chromium's schemeful same-site counts the https app origin
/// [`DEMO_APP_ORIGIN`] and the plain-http port-forward as cross-site, so it never sends the API's
/// `SameSite=Strict` session cookie back and every signed-in read answers 401; https on its own
/// port is same-site with the page, and the API's https-only app origin is unchanged.
const GESTURE_API_ORIGIN: &str = "https://localhost:8788";

/// How `cargo xtask cell demo` runs `web/`: its npm script, where it serves, and the server-side
/// settings it reads (`web/README.md`). Without sign-in it runs on its own fixtures at
/// [`WEB_URL`]; with sign-in it reads the cell's API as the demo owner, through the stub issuer
/// under `next dev` (DEC-2406), at the API's https app origin.
#[derive(Debug, PartialEq, Eq)]
pub struct WebRun {
    pub script: &'static str,
    pub url: &'static str,
    pub env: Vec<(&'static str, String)>,
}

pub fn web_run(signed_in: bool, workspace: &str) -> WebRun {
    if !signed_in {
        return WebRun {
            script: "dev",
            url: WEB_URL,
            env: Vec::new(),
        };
    }
    WebRun {
        script: "dev:cell",
        url: DEMO_APP_ORIGIN,
        env: vec![
            ("OWLHEAD_DATA_SOURCE", "api".to_string()),
            ("OWLHEAD_API_ORIGIN", WEB_API_ORIGIN.to_string()),
            ("OWLHEAD_WORKSPACE_ID", workspace.to_string()),
            ("OWLHEAD_STUB_ISSUER", LOCAL_ISSUER.to_string()),
            ("OWLHEAD_STUB_SUBJECT", DEMO_SUBJECT.to_string()),
        ],
    }
}

/// What a web gesture recorded through the cell's API: the workspace it acted in, the agent, and
/// the command's event id, which the scenario checks on that workspace's control stream.
#[derive(Debug, PartialEq, Eq)]
pub struct WebGesture {
    pub shows: &'static str,
    pub workspace: String,
    pub agent_id: String,
    pub command_id: String,
}

/// What the gesture shows when it ran.
const KILL_SWITCH_SHOWS: &str =
    "the kill switch from web/, pressed in a browser and recorded through the API";
/// How often the driver looks at the Playwright run.
const POLL: Duration = Duration::from_millis(250);
/// The most polls before the run is stopped: 150 seconds, `next dev`'s start included, which the
/// `cell-demo` job's ten minutes hold many times over (DEC-464). Counted, since xtask reads no clock
/// (ES-05).
const GESTURE_POLLS: u32 = 600;
/// The trade loop's polls: 500 seconds, the spec's own waits (a proposal within 120 s, the host's
/// confirmation within 60 s, the fill within 120 s, the kill switch) and `next dev`'s start.
const TRADE_LOOP_POLLS: u32 = 2_000;
/// How long the spec waits for the agent's approval request: 20 seconds, since the runtime decides
/// on the scheduler's tick every second and the browser opens after the gestures' Job, and a loop
/// with no proposal must leave the `cell-demo-web` job its ten minutes (DEC-464).
const PROPOSAL_MS: &str = "20000";
/// How long the spec waits for each fill, the opening's and the owner exit's sell: 60 seconds, as
/// the fixture paper broker fills at once and the executor acts on the scheduler's tick every
/// second. The spec's own 120 s default would put its worst case, two fills among them, past what
/// the `cell-demo-web` job's ten minutes leave after the cell comes up (DEC-464).
const FILL_MS: &str = "60000";
/// Playwright's bound on the whole trade loop run (`OWLHEAD_CELL_TIMEOUT_MS`, the config's
/// `globalTimeout`): 400 seconds, past the spec's `test.setTimeout` of 320 s with `PROPOSAL_MS` and
/// `FILL_MS` (the proposal, the host's 60 s, two fills and its 120 s for the rest) and its two web
/// servers' starts, and inside [`TRADE_LOOP_POLLS`]' 500 s, so Playwright ends the run and writes
/// its result before the driver gives up on it. Unset, the config's 90 s ends every loop before
/// its fill.
const TRADE_LOOP_TIMEOUT_MS: &str = "400000";
/// The trade loop spec under `web/e2e-cell/` (lane W, #2462).
const TRADE_LOOP_SPEC: &str = "trade-loop.spec.ts";
/// The trade loop's steps, in the order the spec reaches them; its result names each one reached.
pub const TRADE_LOOP_STEPS: [&str; 11] = [
    "approval",
    "challenge",
    "approved",
    "order_filled",
    "position",
    "journal",
    "positions_screen",
    "owner_exit",
    "audit_timeline",
    "kill_switch",
    "kill_switch_alert",
];

/// What the trade loop spec asks the host to confirm: the owner's step-up for one approval, the
/// code the page showed for the API's challenge (DEC-2104, DEC-2129).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmRequest {
    pub workspace: String,
    pub principal: String,
    pub agent: String,
    pub approval: String,
    pub challenge: String,
    pub code: String,
}

/// The spec's confirm request file as a [`ConfirmRequest`]; every member must be non-empty text.
///
/// # Errors
/// A file that is not JSON or lacks a member.
pub fn confirm_request(text: &str) -> Result<ConfirmRequest> {
    let value: Value = serde_json::from_str(text).context("the confirm request is not JSON")?;
    let field = |name: &str| {
        value[name]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .with_context(|| format!("the confirm request has no {name}"))
    };
    Ok(ConfirmRequest {
        workspace: field("workspace")?,
        principal: field("principal")?,
        agent: field("agent")?,
        approval: field("approval")?,
        challenge: field("challenge")?,
        code: field("code")?,
    })
}

/// The host's `mandate step-up confirm` for `request`, against the cell's `journal` and `store`.
pub fn confirm_args(request: &ConfirmRequest, journal: &str, store: &str) -> Vec<String> {
    [
        "step-up",
        "confirm",
        "--workspace",
        &request.workspace,
        "--principal",
        &request.principal,
        "--agent",
        &request.agent,
        "--approval",
        &request.approval,
        "--challenge",
        &request.challenge,
        "--code",
        &request.code,
        "--journal",
        journal,
        "--store",
        store,
    ]
    .map(str::to_owned)
    .to_vec()
}

/// What the trade loop reached: each step's recorded value, and why it stopped if it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeLoop {
    pub reached: BTreeMap<String, String>,
    pub error: Option<String>,
}

impl TradeLoop {
    /// Whether every step was reached and nothing stopped it.
    pub fn whole(&self) -> bool {
        self.error.is_none()
            && TRADE_LOOP_STEPS
                .iter()
                .all(|s| self.reached.get(*s).is_some_and(|v| !v.is_empty()))
    }

    /// The first step, in the spec's order, not reached.
    pub fn first_missing(&self) -> Option<&'static str> {
        TRADE_LOOP_STEPS
            .iter()
            .copied()
            .find(|s| self.reached.get(*s).is_none_or(String::is_empty))
    }
}

/// The trade loop's result file: every step it names, and its `error`.
///
/// # Errors
/// A file that is not a JSON object of text.
pub fn trade_loop_result(text: &str) -> Result<TradeLoop> {
    let value: Value = serde_json::from_str(text).context("the trade loop's result is not JSON")?;
    let object = value
        .as_object()
        .context("the trade loop's result is not an object")?;
    let mut reached = BTreeMap::new();
    let mut error = None;
    for (name, member) in object {
        let text = member
            .as_str()
            .with_context(|| format!("the trade loop's {name} is not text"))?
            .to_owned();
        if name == "error" {
            error = Some(text);
        } else {
            reached.insert(name.clone(), text);
        }
    }
    Ok(TradeLoop { reached, error })
}

/// Why the gesture cannot run on this machine, or `None`: it installs nothing and downloads no
/// browser, so each missing piece is the scenario's gap, named.
pub fn gesture_missing(
    npm: bool,
    node_modules: bool,
    chromium: bool,
    openssl: bool,
) -> Option<String> {
    let missing: Vec<&str> = [
        (npm, "npm (web/.nvmrc names the Node.js release)"),
        (node_modules, "web/node_modules (`npm ci` in web/)"),
        (chromium, "Playwright's Chromium (PLAYWRIGHT_BROWSERS_PATH)"),
        (openssl, "openssl, for next dev's local certificate"),
    ]
    .into_iter()
    .filter(|(present, _)| !present)
    .map(|(_, what)| what)
    .collect();
    (!missing.is_empty()).then(|| format!("web/'s gesture needs {}", missing.join(", ")))
}

/// The gesture's result file as its outcome: the recorded command, or the reason it pressed nothing.
pub fn gesture_result(text: &str) -> Result<WebGesture> {
    let result: Value = serde_json::from_str(text).context("the gesture's result is not JSON")?;
    if let Some(error) = result["error"].as_str() {
        bail!("{error}");
    }
    let field = |name: &str| {
        result[name]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .with_context(|| format!("the gesture's result has no {name}"))
    };
    Ok(WebGesture {
        shows: KILL_SWITCH_SHOWS,
        workspace: field("workspace")?,
        agent_id: field("agent_id")?,
        command_id: field("command_id")?,
    })
}

/// Why no web gesture can run on this machine, or `None`: [`gesture_missing`] over what this
/// machine has, so a caller can tell a machine without the tools from a gesture that failed.
///
/// # Errors
/// When the repository's root cannot be found.
pub fn unavailable() -> Result<Option<String>> {
    let web = crate::repo_root()?.join("web");
    Ok(gesture_missing(
        answers("npm", "--version"),
        web.join("node_modules").exists(),
        chromium_cached(),
        answers("openssl", "version"),
    ))
}

/// Whether Playwright's Chromium is already on this machine, where `PLAYWRIGHT_BROWSERS_PATH` or
/// its default cache puts it.
fn chromium_cached() -> bool {
    let cache = env::var_os("PLAYWRIGHT_BROWSERS_PATH")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(".cache/ms-playwright")));
    cache
        .and_then(|dir| fs::read_dir(dir).ok())
        .is_some_and(|mut entries| {
            entries
                .any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("chromium")))
        })
}

fn answers(program: &str, arg: &str) -> bool {
    Command::new(program)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// How many polls the group gets to stop on SIGTERM before it is killed: five seconds.
const STOP_POLLS: u32 = 20;
/// How often, and how many times, the group is looked at after SIGKILL, which is delivered
/// asynchronously: up to two seconds for its last process to go.
const KILL_POLL: Duration = Duration::from_millis(100);
const KILL_POLLS: u32 = 20;

/// A process and everything it starts, in a process group of their own: `npx`, Playwright, and the
/// `next dev` server Playwright starts. Dropping it, on success, timeout, error, or unwinding, stops
/// the whole group, SIGTERM first so Playwright can stop its server, then SIGKILL, so no server is
/// left holding the app origin's port for the next run.
pub struct Group {
    child: Child,
    pgid: u32,
    stopped: bool,
}

impl Group {
    /// Spawns `command` as the leader of a new process group.
    pub fn spawn(command: &mut Command) -> Result<Group> {
        let child = command.process_group(0).spawn()?;
        let pgid = child.id();
        Ok(Group {
            child,
            pgid,
            stopped: false,
        })
    }

    /// The leader's exit status, once it has one within `polls` polls, `None` when it ran past them,
    /// calling `each` before every poll so a driver can answer the run while it waits. An error from
    /// `each` stops the wait with it.
    pub fn wait_answering(
        &mut self,
        polls: u32,
        each: &mut dyn FnMut() -> Result<()>,
    ) -> Result<(Option<ExitStatus>, u32)> {
        for polled in 0..=polls {
            if let Some(status) = self.child.try_wait()? {
                return Ok((Some(status), polled));
            }
            each()?;
            if polled < polls {
                sleep(POLL);
            }
        }
        Ok((None, polls))
    }

    /// Whether any process of the group is still alive.
    pub fn alive(&self) -> bool {
        group_alive(self.pgid)
    }

    /// Stops every process in the group: SIGTERM, up to [`STOP_POLLS`] polls, then SIGKILL, reaps
    /// the leader, and waits up to [`KILL_POLLS`] polls for the group to be empty. It returns the
    /// leader's status (`None` once already stopped), or an error naming the group if a process
    /// of it is still alive at the deadline. Idempotent.
    pub fn stop(&mut self) -> Result<Option<ExitStatus>> {
        if self.stopped {
            return Ok(None);
        }
        self.stopped = true;
        signal("-TERM", self.pgid);
        for _ in 0..STOP_POLLS {
            let _ = self.child.try_wait();
            if !self.alive() {
                break;
            }
            sleep(POLL);
        }
        signal("-KILL", self.pgid);
        let leader = self.child.wait().ok();
        for _ in 0..KILL_POLLS {
            if !self.alive() {
                return Ok(leader);
            }
            sleep(KILL_POLL);
        }
        if self.alive() {
            bail!(
                "process group {} still has a live process {}s after SIGKILL",
                self.pgid,
                (KILL_POLL * KILL_POLLS).as_secs()
            );
        }
        Ok(leader)
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Whether a live process is in group `pgid`. Where `/proc` exists, a zombie does not count: a
/// container's first process may never reap the group's orphans, and `kill -0` still reaches a
/// zombie. Elsewhere, whether `kill -0` reaches the group.
fn group_alive(pgid: u32) -> bool {
    let Ok(procs) = fs::read_dir("/proc") else {
        return signal("-0", pgid);
    };
    procs.flatten().any(|entry| {
        fs::read_to_string(entry.path().join("stat")).is_ok_and(|stat| {
            stat.rsplit_once(") ").is_some_and(|(_, rest)| {
                let fields: Vec<&str> = rest.split_whitespace().collect();
                fields.first().is_some_and(|state| *state != "Z")
                    && fields
                        .get(2)
                        .is_some_and(|group| *group == pgid.to_string())
            })
        })
    })
}

/// Sends `sig` to the process group `pgid` with the system's `kill`; whether it reached a process.
fn signal(sig: &str, pgid: u32) -> bool {
    Command::new("kill")
        .args([sig, "--", &format!("-{pgid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Step 7 from `web/`, in `workspace`, the one the cell's API hosts (the scenario passes its own run's,
/// never a fixed id), and to `agent` when it names one (else the first listed agent not stopped; a
/// named agent already stopped is an `Err`, since D10 offers nothing to confirm for it): starts
/// `next dev` against the cell's API as the demo owner, signs in through
/// the stub, opens Stop and presses one listed agent's kill switch, and returns what the API
/// recorded. An `Err` is the gap the scenario prints: a missing tool, no agent to kill, or a run
/// that failed or ran past [`GESTURE_POLLS`] (its log is named).
pub fn kill_switch_from_web(workspace: &str, agent: Option<&str>) -> Result<WebGesture> {
    let run = spec_run(
        workspace,
        &[("OWLHEAD_CELL_AGENT", agent.unwrap_or_default().to_owned())],
        "web-gesture",
        GESTURE_POLLS,
        &mut || Ok(()),
    )?;
    match (run.status.success(), run.result) {
        (_, Some(text)) if text.contains("\"error\"") => gesture_result(&text),
        (true, Some(text)) => gesture_result(&text),
        _ => bail!(
            "web/'s gesture failed ({}); its log is {}",
            run.status,
            run.log.display()
        ),
    }
}

/// The trade loop from `web/` (lane W's `trade-loop.spec.ts`, #2462) on `agent` in `workspace`:
/// the owner, signed in, waits for the agent's approval request, approves it with the code the
/// page shows, which the driver confirms on the host as the owner would, by calling `confirm` with
/// the spec's request, then sees the order filled, the position and the journal, and presses the
/// agent's kill switch. Its result names every step reached; a run that stopped is a
/// [`TradeLoop`] with `error`, not an `Err`. An `Err` is a missing tool, a run past
/// [`TRADE_LOOP_POLLS`], or a run that wrote no result (its log is named).
pub fn trade_loop_from_web(
    workspace: &str,
    agent: &str,
    confirm: &mut dyn FnMut(&ConfirmRequest) -> std::result::Result<(), String>,
) -> Result<TradeLoop> {
    let dir = crate::repo_root()?.join(state()?).join("web-trade-loop");
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let (request, done) = (
        dir.join("confirm-request.json"),
        dir.join("confirm-done.json"),
    );
    let _ = fs::remove_file(&request);
    let _ = fs::remove_file(&done);
    let mut answered = false;
    let mut answer = || -> Result<()> {
        if answered {
            return Ok(());
        }
        let Ok(text) = fs::read_to_string(&request) else {
            return Ok(());
        };
        answered = true;
        let reply = match confirm_request(&text) {
            Ok(asked) => match confirm(&asked) {
                Ok(()) => serde_json::json!({"ok": true}),
                Err(why) => serde_json::json!({"ok": false, "error": why}),
            },
            Err(why) => serde_json::json!({"ok": false, "error": format!("{why:#}")}),
        };
        fs::write(&done, reply.to_string()).with_context(|| format!("writing {}", done.display()))
    };
    let run = spec_run(
        workspace,
        &trade_loop_env(agent, &request, &done),
        "web-trade-loop",
        TRADE_LOOP_POLLS,
        &mut answer,
    )?;
    match run.result {
        Some(text) => trade_loop_result(&text),
        None => bail!(
            "web/'s trade loop wrote no result ({}); its log is {}",
            run.status,
            run.log.display()
        ),
    }
}

/// What the trade loop's Playwright run is handed beside the shared environment: the spec, the
/// agent, the host's confirmation files, the proposal's wait, and the run's bound.
fn trade_loop_env(agent: &str, request: &Path, done: &Path) -> [(&'static str, String); 7] {
    [
        ("OWLHEAD_CELL_SPEC", TRADE_LOOP_SPEC.to_owned()),
        ("OWLHEAD_CELL_AGENT", agent.to_owned()),
        ("OWLHEAD_CELL_CONFIRM_REQUEST", path_text(request)),
        ("OWLHEAD_CELL_CONFIRM_DONE", path_text(done)),
        ("OWLHEAD_CELL_PROPOSAL_MS", PROPOSAL_MS.to_owned()),
        ("OWLHEAD_CELL_FILL_MS", FILL_MS.to_owned()),
        ("OWLHEAD_CELL_TIMEOUT_MS", TRADE_LOOP_TIMEOUT_MS.to_owned()),
    ]
}

/// The customer journey spec under `web/e2e-cell/` (lane GL, DEC-2134).
const JOURNEY_SPEC: &str = "customer-journey.spec.ts";
/// Playwright's bound on the journey run (`OWLHEAD_CELL_TIMEOUT_MS`): 330 seconds, which holds the
/// spec's own 240 s and `next dev`'s 60 s start with room to spare, and ends inside
/// [`TRADE_LOOP_POLLS`]' 500 s, so Playwright writes its result before the driver gives up.
const JOURNEY_TIMEOUT_MS: &str = "330000";
/// The customer journey's steps, in the order the spec reaches them; its result names each one.
pub const JOURNEY_STEPS: [&str; 5] = [
    "connections",
    "disclosures",
    "disclosure_accepted",
    "real_money_refused",
    "refusal_journaled",
];

/// The customer's journey from `web/` (`customer-journey.spec.ts`, DEC-2134) on `agent` in
/// `workspace`, after the demo scenario: the owner, signed in, sees Alpaca paper connected and
/// Robinhood's real money off, reads the go-live screen's disclosures and limits pending counsel,
/// asks to trade real money and is refused, and finds the refusal journaled. Answers the steps
/// reached; an `Err` names the first that was not, or a run that wrote no result.
///
/// # Errors
/// A missing tool, a run past [`GESTURE_POLLS`], a run that wrote no result, or a step not reached.
pub fn journey_from_web(workspace: &str, agent: &str) -> Result<BTreeMap<String, String>> {
    let run = spec_run(
        workspace,
        &[
            ("OWLHEAD_CELL_SPEC", JOURNEY_SPEC.to_owned()),
            ("OWLHEAD_CELL_AGENT", agent.to_owned()),
            ("OWLHEAD_CELL_TIMEOUT_MS", JOURNEY_TIMEOUT_MS.to_owned()),
        ],
        "web-journey",
        TRADE_LOOP_POLLS,
        &mut || Ok(()),
    )?;
    let Some(text) = run.result else {
        bail!(
            "web/'s customer journey wrote no result ({}); its log is {}",
            run.status,
            run.log.display()
        );
    };
    journey_result(&text)
}

/// The journey spec's result file as the steps it reached; an `error`, or a step of
/// [`JOURNEY_STEPS`] missing, is an `Err` naming it.
///
/// # Errors
/// A result that is not a JSON object of strings, names an error, or lacks a step.
pub fn journey_result(text: &str) -> Result<BTreeMap<String, String>> {
    let steps: BTreeMap<String, String> = serde_json::from_str(text)
        .context("the journey's result is not a JSON object of strings")?;
    if let Some(error) = steps.get("error") {
        bail!("the customer journey stopped: {error}");
    }
    if let Some(missing) = JOURNEY_STEPS.iter().find(|s| !steps.contains_key(**s)) {
        bail!("the customer journey did not reach `{missing}`");
    }
    Ok(steps)
}

/// The owner's set-up spec under `web/e2e-cell/` (the demo's step 2 from web/, lane WB).
const SETUP_SPEC: &str = "setup.spec.ts";
/// Playwright's bound on the set-up run (`OWLHEAD_CELL_TIMEOUT_MS`): 330 seconds, which holds the
/// spec's own 240 s and `next dev`'s 60 s start with room to spare, and ends inside
/// [`TRADE_LOOP_POLLS`]' 500 s, so Playwright writes its result before the driver gives up.
const SETUP_TIMEOUT_MS: &str = "330000";
/// The set-up's steps, in the order the spec reaches them; its result names each one.
pub const SETUP_STEPS: [&str; 4] = [
    "passkey_enrolled",
    "version_created",
    "confirm_refused_cooling_off",
    "deploy_not_offered",
];

/// The owner's set-up from `web/` (`setup.spec.ts`) in `workspace`: the owner, signed in, adds a
/// passkey in Settings through the API's enrolment, made by Chromium's virtual authenticator,
/// creates version 1 in `/agents/new`, and is told the confirm waits for the new passkey's first 24
/// hours, with nothing sent and no deployment offered. A first passkey's cool-off is a safety rule
/// the founder decides (DEC-79), so the cell's confirm and deploy stay the CLI's. Answers the steps
/// reached; an `Err` names the first that was not, or a run that wrote no result.
///
/// # Errors
/// A missing tool, a run past [`TRADE_LOOP_POLLS`], a run that wrote no result, or a step not
/// reached.
pub fn setup_from_web(workspace: &str) -> Result<BTreeMap<String, String>> {
    let run = spec_run(
        workspace,
        &[
            ("OWLHEAD_CELL_SPEC", SETUP_SPEC.to_owned()),
            ("OWLHEAD_CELL_TIMEOUT_MS", SETUP_TIMEOUT_MS.to_owned()),
        ],
        "web-setup",
        TRADE_LOOP_POLLS,
        &mut || Ok(()),
    )?;
    let Some(text) = run.result else {
        bail!(
            "web/'s set-up wrote no result ({}); its log is {}",
            run.status,
            run.log.display()
        );
    };
    setup_result(&text)
}

/// The set-up spec's result file as the steps it reached; an `error`, or a step of
/// [`SETUP_STEPS`] missing, is an `Err` naming it.
///
/// # Errors
/// A result that is not a JSON object of strings, names an error, or lacks a step.
pub fn setup_result(text: &str) -> Result<BTreeMap<String, String>> {
    let steps: BTreeMap<String, String> = serde_json::from_str(text)
        .context("the set-up's result is not a JSON object of strings")?;
    if let Some(error) = steps.get("error") {
        bail!("the set-up from web/ stopped: {error}");
    }
    if let Some(missing) = SETUP_STEPS.iter().find(|s| !steps.contains_key(**s)) {
        bail!("the set-up from web/ did not reach `{missing}`");
    }
    Ok(steps)
}

/// The backtest report spec under `web/e2e-cell/` (the demo's step 5 from web/, lane W).
const REPORT_SPEC: &str = "backtest-report.spec.ts";
/// Playwright's bound on the report run (`OWLHEAD_CELL_TIMEOUT_MS`): 270 seconds, which holds the
/// spec's own 180 s and `next dev`'s 60 s start with room to spare, and ends inside
/// [`TRADE_LOOP_POLLS`]' 500 s, so Playwright writes its result before the driver gives up.
const REPORT_TIMEOUT_MS: &str = "270000";
/// The report visit's steps, in the order the spec reaches them; its result names each one.
pub const REPORT_STEPS: [&str; 2] = ["report_link", "report_shown"];

/// The owner's visit of the backtest `backtest`'s report from `web/` (`backtest-report.spec.ts`) in
/// `workspace`: the owner, signed in, opens its link, `/backtests/{id}`, since the API lists no
/// backtests, and sees the report the runner stored, read through the API. Needs no market session.
/// Answers the steps reached, `report_link` naming the link; an `Err` names the first step that was
/// not reached, or a run that wrote no result.
///
/// # Errors
/// A missing tool, a run past [`TRADE_LOOP_POLLS`], a run that wrote no result, or a step not
/// reached.
pub fn report_from_web(workspace: &str, backtest: &str) -> Result<BTreeMap<String, String>> {
    let run = spec_run(
        workspace,
        &[
            ("OWLHEAD_CELL_SPEC", REPORT_SPEC.to_owned()),
            ("OWLHEAD_CELL_BACKTEST", backtest.to_owned()),
            ("OWLHEAD_CELL_TIMEOUT_MS", REPORT_TIMEOUT_MS.to_owned()),
        ],
        "web-report",
        TRADE_LOOP_POLLS,
        &mut || Ok(()),
    )?;
    let Some(text) = run.result else {
        bail!(
            "web/'s report visit wrote no result ({}); its log is {}",
            run.status,
            run.log.display()
        );
    };
    report_result(&text)
}

/// The report spec's result file as the steps it reached; an `error`, or a step of
/// [`REPORT_STEPS`] missing, is an `Err` naming it.
///
/// # Errors
/// A result that is not a JSON object of strings, names an error, or lacks a step.
pub fn report_result(text: &str) -> Result<BTreeMap<String, String>> {
    let steps: BTreeMap<String, String> = serde_json::from_str(text)
        .context("the report visit's result is not a JSON object of strings")?;
    if let Some(error) = steps.get("error") {
        bail!("the report visit from web/ stopped: {error}");
    }
    if let Some(missing) = REPORT_STEPS.iter().find(|s| !steps.contains_key(**s)) {
        bail!("the report visit from web/ did not reach `{missing}`");
    }
    Ok(steps)
}

/// Whether lane W's trade loop spec is in this checkout (#2462).
///
/// # Errors
/// When the repository's root cannot be found.
pub fn trade_loop_spec_present() -> Result<bool> {
    Ok(crate::repo_root()?
        .join("web/e2e-cell")
        .join(TRADE_LOOP_SPEC)
        .is_file())
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

/// One finished Playwright run of a cell spec: its status, its result file's text, and its log.
struct SpecRun {
    status: ExitStatus,
    result: Option<String>,
    log: PathBuf,
}

/// Runs a cell spec against the cell's API as the demo owner, signed in through the stub, with
/// `extra` beside the shared environment, under `state/<dir>`, for at most `polls` polls, calling
/// `each` before every poll.
fn spec_run(
    workspace: &str,
    extra: &[(&str, String)],
    dir: &str,
    polls: u32,
    each: &mut dyn FnMut() -> Result<()>,
) -> Result<SpecRun> {
    if let Some(why) = unavailable()? {
        bail!(why);
    }
    let web = crate::repo_root()?.join("web");
    let dir = crate::repo_root()?.join(state()?).join(dir);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let (key, cert, result, log) = (
        dir.join("key.pem"),
        dir.join("cert.pem"),
        dir.join("result.json"),
        dir.join("playwright.log"),
    );
    let _ = fs::remove_file(&result);
    let made = Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
        ])
        .args([
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost",
        ])
        .arg("-keyout")
        .arg(&key)
        .arg("-out")
        .arg(&cert)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("making next dev's local certificate")?;
    if !made.success() {
        bail!("openssl could not make next dev's local certificate");
    }
    let output = fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
    let mut group = Group::spawn(
        Command::new("npx")
            .current_dir(&web)
            .args(["playwright", "test", "-c", "playwright.cell.config.ts"])
            .envs(
                web_run(true, workspace)
                    .env
                    .iter()
                    .map(|(n, v)| (*n, v.as_str())),
            )
            .env("OWLHEAD_API_ORIGIN", GESTURE_API_ORIGIN)
            .env("OWLHEAD_CELL_API_UPSTREAM", WEB_API_ORIGIN)
            .env("OWLHEAD_CELL_TLS_KEY", &key)
            .env("OWLHEAD_CELL_TLS_CERT", &cert)
            .env("OWLHEAD_CELL_RESULT", &result)
            .envs(extra.iter().map(|(n, v)| (*n, v.as_str())))
            .stdout(output.try_clone()?)
            .stderr(output),
    )
    .context("starting web/'s Playwright run")?;
    let waited = group.wait_answering(polls, each);
    group.stop()?;
    let (status, polled) = waited?;
    let Some(status) = status else {
        bail!(
            "web/'s run ran past {}s and was stopped; its log is {}",
            (POLL * polls).as_secs(),
            log.display()
        );
    };
    eprintln!(
        "    web/: the Playwright run took about {:.1}s",
        (POLL * polled).as_secs_f64()
    );
    Ok(SpecRun {
        status,
        result: fs::read_to_string(&result).ok(),
        log,
    })
}

/// Prints what the gesture recorded, for `cargo xtask cell web-kill-switch`.
pub fn report(gesture: Result<WebGesture>) -> Result<()> {
    let gesture = gesture?;
    eprintln!(
        "==> {}: agent {} in workspace {}, command {}",
        gesture.shows, gesture.agent_id, gesture.workspace, gesture.command_id
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use anyhow::{Context, Result};
    use serde_json::Value;

    use std::os::unix::process::ExitStatusExt;

    use super::super::{API_LOCAL_PORT, DEMO_WORKSPACE, STUB_PORT};
    use super::*;

    /// The trade loop spec's test timeout, as `trade-loop.spec.ts` sets it from its waits.
    const TRADE_LOOP_SPEC_TIMEOUT: &str =
        "test.setTimeout(PROPOSAL_MS + CONFIRM_MS + 2 * FILL_MS + 120_000);";
    /// The host's confirmation wait, as `trade-loop.spec.ts` defaults it; the driver hands none.
    const TRADE_LOOP_CONFIRM_MS: u64 = 60_000;
    /// The 120 s the trade loop spec allows the rest of its steps, as its test timeout adds it.
    const TRADE_LOOP_REST_MS: u64 = 120_000;
    /// The longest the TLS front takes to start, as `playwright.cell.config.ts`'s `webServer`
    /// bounds it, beside `next dev`'s [`NEXT_DEV_START_MS`].
    const TLS_FRONT_START_MS: u64 = 10_000;
    /// The longest `next dev` takes to start, as `playwright.cell.config.ts`'s `webServer` bounds it.
    const NEXT_DEV_START_MS: u64 = 60_000;
    /// The journey's result passes only when it names every step and no error, and the spec it
    /// runs is in this checkout and writes each step the driver reads.
    #[test]
    fn the_journey_reads_every_step_and_names_the_first_one_missing() -> Result<()> {
        let all = r#"{"connections":"ok","disclosures":"4","disclosure_accepted":"ok","real_money_refused":"ok","refusal_journaled":"01JB"}"#;
        assert_eq!(journey_result(all)?.len(), 5);
        let stopped = r#"{"connections":"ok","error":"no Alpaca connection is listed"}"#;
        let why = format!("{:#}", journey_result(stopped).err().context("an error")?);
        assert!(why.contains("no Alpaca connection is listed"), "{why}");
        let short = r#"{"connections":"ok","disclosures":"4","disclosure_accepted":"ok","real_money_refused":"ok"}"#;
        let why = format!(
            "{:#}",
            journey_result(short).err().context("a missing step")?
        );
        assert!(why.contains("`refusal_journaled`"), "{why}");
        assert!(journey_result("[]").is_err());
        let spec = fs::read_to_string(crate::repo_root()?.join("web/e2e-cell").join(JOURNEY_SPEC))?;
        for step in JOURNEY_STEPS {
            assert!(
                spec.contains(&format!("step(\"{step}\"")),
                "the spec writes {step}"
            );
        }
        Ok(())
    }

    /// The set-up's result passes only when it names every step and no error, and the spec it
    /// runs is in this checkout and writes each step the driver reads.
    #[test]
    fn the_setup_reads_every_step_and_names_the_first_one_missing() -> Result<()> {
        let all = r#"{"passkey_enrolled":"pk","version_created":"sha256:ab","confirm_refused_cooling_off":"ok","deploy_not_offered":"ok"}"#;
        assert_eq!(setup_result(all)?.len(), 4);
        let stopped =
            r#"{"passkey_enrolled":"pk","error":"the deployment registered no instrument"}"#;
        let why = format!("{:#}", setup_result(stopped).err().context("an error")?);
        assert!(
            why.contains("the deployment registered no instrument"),
            "{why}"
        );
        let short = r#"{"passkey_enrolled":"pk","version_created":"sha256:ab","confirm_refused_cooling_off":"ok"}"#;
        let why = format!("{:#}", setup_result(short).err().context("a missing step")?);
        assert!(why.contains("`deploy_not_offered`"), "{why}");
        assert!(setup_result("[]").is_err());
        let spec = fs::read_to_string(crate::repo_root()?.join("web/e2e-cell").join(SETUP_SPEC))?;
        for step in SETUP_STEPS {
            assert!(
                spec.contains(&format!("step(\"{step}\"")),
                "the spec writes {step}"
            );
        }
        Ok(())
    }

    /// The report visit's result is every step or the first one missing, and the spec writes each.
    #[test]
    fn the_report_visit_reads_every_step_and_names_the_first_one_missing() -> Result<()> {
        let whole = r#"{"report_link":"/backtests/bt-demo-01","report_shown":"ok"}"#;
        assert_eq!(
            report_result(whole)?.get("report_link").map(String::as_str),
            Some("/backtests/bt-demo-01")
        );
        let stopped = r#"{"report_link":"/backtests/bt-demo-01","error":"no comparison shown"}"#;
        let why = format!("{:#}", report_result(stopped).err().context("an error")?);
        assert!(why.contains("no comparison shown"), "{why}");
        let short = r#"{"report_link":"/backtests/bt-demo-01"}"#;
        let why = format!(
            "{:#}",
            report_result(short).err().context("a missing step")?
        );
        assert!(why.contains("`report_shown`"), "{why}");
        assert!(report_result("[]").is_err());
        let spec = fs::read_to_string(crate::repo_root()?.join("web/e2e-cell").join(REPORT_SPEC))?;
        for step in REPORT_STEPS {
            assert!(
                spec.contains(&format!("step(\"{step}\"")),
                "the spec writes {step}"
            );
        }
        Ok(())
    }

    /// The report spec's own test timeout, as `backtest-report.spec.ts` defaults `REPORT_MS`.
    const REPORT_SPEC_MS: u64 = 180_000;

    /// Playwright's bound on the report visit holds the spec's own test timeout and `next dev`'s
    /// start, with margin, and ends before the driver's polls do.
    #[test]
    fn the_report_visits_playwright_bound_holds_the_spec_and_ends_before_the_polls() -> Result<()> {
        let spec = fs::read_to_string(crate::repo_root()?.join("web/e2e-cell").join(REPORT_SPEC))?;
        assert!(
            spec.contains(&format!(
                "?? {}_{:03})",
                REPORT_SPEC_MS / 1000,
                REPORT_SPEC_MS % 1000
            )),
            "the spec defaults REPORT_MS to {REPORT_SPEC_MS} ms"
        );
        let bound: u64 = REPORT_TIMEOUT_MS.parse()?;
        let polls_ms = u64::from(TRADE_LOOP_POLLS) * u64::try_from(POLL.as_millis())?;
        assert!(
            bound > REPORT_SPEC_MS + NEXT_DEV_START_MS,
            "{bound} ms leaves no margin past the spec's {REPORT_SPEC_MS} ms and next dev's start"
        );
        assert!(bound < polls_ms, "{bound} ms outlasts the driver's polls");
        Ok(())
    }

    /// The set-up spec's own test timeout, as `setup.spec.ts` defaults `SETUP_MS`.
    const SETUP_SPEC_MS: u64 = 240_000;

    /// Playwright's bound on the set-up holds the spec's own test timeout and `next dev`'s start,
    /// with margin, and ends before the driver's polls do, so a slow set-up is the spec's named
    /// failure rather than Playwright's global kill.
    #[test]
    fn the_setups_playwright_bound_holds_the_spec_and_ends_before_the_polls() -> Result<()> {
        let spec = fs::read_to_string(crate::repo_root()?.join("web/e2e-cell").join(SETUP_SPEC))?;
        assert!(
            spec.contains(&format!(
                "?? {}_{:03})",
                SETUP_SPEC_MS / 1000,
                SETUP_SPEC_MS % 1000
            )),
            "the spec defaults SETUP_MS to {SETUP_SPEC_MS} ms"
        );
        let bound: u64 = SETUP_TIMEOUT_MS.parse()?;
        let polls_ms = u64::from(TRADE_LOOP_POLLS) * u64::try_from(POLL.as_millis())?;
        assert!(
            bound > SETUP_SPEC_MS + NEXT_DEV_START_MS,
            "{bound} ms leaves no margin past the spec's {SETUP_SPEC_MS} ms and next dev's start"
        );
        assert!(bound < polls_ms, "{bound} ms outlasts the driver's polls");
        Ok(())
    }

    /// Playwright's bound on the trade loop holds the spec's own test timeout, as the spec computes
    /// it from the waits the driver hands it and its own defaults, and both web servers' starts,
    /// and ends before the driver's polls do, so a slow loop is the spec's named failure with a
    /// result rather than Playwright's global kill or a run the driver kills (#2466's and #2871's
    /// reviews). The formula and the defaults are read from the spec and the config, so a wait the
    /// spec adds fails this test until the bound holds it.
    #[test]
    fn the_trade_loops_playwright_bound_holds_the_spec_and_ends_before_the_polls() -> Result<()> {
        let root = crate::repo_root()?;
        let spec = fs::read_to_string(root.join("web/e2e-cell").join(TRADE_LOOP_SPEC))?;
        let config = fs::read_to_string(root.join("web/playwright.cell.config.ts"))?;
        assert!(
            spec.contains(TRADE_LOOP_SPEC_TIMEOUT),
            "the spec's test timeout is {TRADE_LOOP_SPEC_TIMEOUT}"
        );
        assert!(
            spec.contains("process.env.OWLHEAD_CELL_CONFIRM_MS ?? 60_000)"),
            "the spec defaults CONFIRM_MS to {TRADE_LOOP_CONFIRM_MS} ms"
        );
        for start in [TLS_FRONT_START_MS, NEXT_DEV_START_MS] {
            assert!(
                config.contains(&format!("timeout: {}_{:03},", start / 1000, start % 1000)),
                "the config bounds a web server's start at {start} ms"
            );
        }
        let env = trade_loop_env("agent", Path::new("request"), Path::new("done"));
        let handed = |name: &str| -> Result<u64> {
            let value = env
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, value)| value.as_str())
                .with_context(|| format!("the run is handed no {name}"))?;
            Ok(value.parse()?)
        };
        assert!(
            env.iter().all(|(n, _)| *n != "OWLHEAD_CELL_CONFIRM_MS"),
            "the confirmation wait is the spec's default"
        );
        let bound = handed("OWLHEAD_CELL_TIMEOUT_MS")?;
        let test_timeout = handed("OWLHEAD_CELL_PROPOSAL_MS")?
            + TRADE_LOOP_CONFIRM_MS
            + 2 * handed("OWLHEAD_CELL_FILL_MS")?
            + TRADE_LOOP_REST_MS;
        let polls_ms = u64::from(TRADE_LOOP_POLLS) * u64::try_from(POLL.as_millis())?;
        assert_eq!(
            polls_ms, 500_000,
            "the driver's polls, as their doc gives them"
        );
        assert_eq!(
            test_timeout, 320_000,
            "the spec's test timeout, as the bound's doc gives it"
        );
        assert!(
            bound >= test_timeout + TLS_FRONT_START_MS + NEXT_DEV_START_MS,
            "{bound} ms cannot hold the spec's {test_timeout} ms and its web servers' starts"
        );
        assert!(bound < polls_ms, "{bound} ms outlasts the driver's polls");
        Ok(())
    }

    /// The journey spec's own test timeout, as `customer-journey.spec.ts` defaults `JOURNEY_MS`.
    const JOURNEY_SPEC_MS: u64 = 240_000;

    /// Playwright's bound on the journey holds the spec's own test timeout and `next dev`'s start,
    /// with margin, and ends before the driver's polls do, so a slow journey is the spec's named
    /// failure rather than Playwright's global kill (#2589's review).
    #[test]
    fn the_journeys_playwright_bound_holds_the_spec_and_ends_before_the_polls() -> Result<()> {
        let spec = fs::read_to_string(crate::repo_root()?.join("web/e2e-cell").join(JOURNEY_SPEC))?;
        assert!(
            spec.contains(&format!(
                "?? {}_{:03})",
                JOURNEY_SPEC_MS / 1000,
                JOURNEY_SPEC_MS % 1000
            )),
            "the spec defaults JOURNEY_MS to {JOURNEY_SPEC_MS} ms"
        );
        let bound: u64 = JOURNEY_TIMEOUT_MS.parse()?;
        let polls_ms = u64::from(TRADE_LOOP_POLLS) * u64::try_from(POLL.as_millis())?;
        assert!(
            bound > JOURNEY_SPEC_MS + NEXT_DEV_START_MS,
            "{bound} ms leaves no margin past the spec's {JOURNEY_SPEC_MS} ms and next dev's start"
        );
        assert!(bound < polls_ms, "{bound} ms outlasts the driver's polls");
        Ok(())
    }

    /// The trade loop's request is read whole, every member non-empty; the host's confirmation
    /// passes exactly its members and the cell's journal and store; the result names the steps
    /// reached, and the first one missing is the one after the last reached.
    #[test]
    fn the_trade_loop_reads_its_request_and_result_and_confirms_with_their_members() -> Result<()> {
        let text = r#"{"workspace":"W","principal":"P","agent":"A","approval":"AP","challenge":"C","code":"ABCD-EFGH"}"#;
        let asked = confirm_request(text)?;
        assert_eq!(
            confirm_args(&asked, "$(DSN)", "/store"),
            [
                "step-up",
                "confirm",
                "--workspace",
                "W",
                "--principal",
                "P",
                "--agent",
                "A",
                "--approval",
                "AP",
                "--challenge",
                "C",
                "--code",
                "ABCD-EFGH",
                "--journal",
                "$(DSN)",
                "--store",
                "/store"
            ]
        );
        for member in [
            "workspace",
            "principal",
            "agent",
            "approval",
            "challenge",
            "code",
        ] {
            let mut value: Value = serde_json::from_str(text)?;
            value[member] = Value::String(String::new());
            assert!(
                confirm_request(&value.to_string()).is_err(),
                "{member} empty"
            );
            value.as_object_mut().context("an object")?.remove(member);
            assert!(
                confirm_request(&value.to_string()).is_err(),
                "{member} missing"
            );
        }
        assert!(confirm_request("not json").is_err());
        let partial =
            trade_loop_result(r#"{"approval":"AP1","challenge":"C1","error":"no code"}"#)?;
        assert_eq!(partial.first_missing(), Some("approved"));
        assert_eq!(partial.error.as_deref(), Some("no code"));
        assert!(!partial.whole());
        let mut all: BTreeMap<String, String> = TRADE_LOOP_STEPS
            .iter()
            .map(|s| ((*s).to_owned(), "x".to_owned()))
            .collect();
        let whole = trade_loop_result(&serde_json::to_string(&all)?)?;
        assert!(whole.whole());
        assert_eq!(whole.first_missing(), None);
        all.insert("order_filled".into(), String::new());
        let empty = trade_loop_result(&serde_json::to_string(&all)?)?;
        assert_eq!(
            empty.first_missing(),
            Some("order_filled"),
            "an empty value is not reached"
        );
        assert!(trade_loop_result(r#"{"approval":3}"#).is_err());
        assert!(trade_loop_result("[]").is_err());
        Ok(())
    }

    /// The wait answers the run before every poll, and an answer's error stops the wait.
    #[test]
    fn the_wait_answers_before_every_poll_and_stops_on_an_error() -> Result<()> {
        let mut group = Group::spawn(Command::new("sleep").arg("30"))?;
        let mut asked = 0;
        let (status, polls) = group.wait_answering(3, &mut || {
            asked += 1;
            Ok(())
        })?;
        assert_eq!((status, polls, asked), (None, 3, 4));
        let stopped = group.wait_answering(3, &mut || anyhow::bail!("no"));
        assert!(stopped.is_err());
        group.stop()?;
        Ok(())
    }

    #[test]
    fn web_reads_the_cells_api_as_the_demo_owner_only_once_sign_in_is_ready() -> Result<()> {
        let fixtures = web_run(false, DEMO_WORKSPACE);
        assert_eq!(
            (fixtures.script, fixtures.url, fixtures.env.len()),
            ("dev", WEB_URL, 0),
            "without sign-in web/ runs on its own fixtures"
        );
        let cell = web_run(true, DEMO_WORKSPACE);
        assert_eq!(
            cell.url, DEMO_APP_ORIGIN,
            "web/ serves at the one origin the API's CORS and CSRF checks admit"
        );
        let env: BTreeMap<&str, &str> = cell.env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(
            env,
            BTreeMap::from([
                ("OWLHEAD_DATA_SOURCE", "api"),
                ("OWLHEAD_API_ORIGIN", "http://localhost:8787"),
                ("OWLHEAD_WORKSPACE_ID", DEMO_WORKSPACE),
                ("OWLHEAD_STUB_ISSUER", LOCAL_ISSUER),
                ("OWLHEAD_STUB_SUBJECT", DEMO_SUBJECT),
            ]),
            "web/'s settings name the forwarded API, the demo workspace, and the stub's issuer and subject"
        );
        assert!(
            WEB_API_ORIGIN.ends_with(&format!(":{API_LOCAL_PORT}")),
            "web/ reaches the API on the port the demo forwards"
        );
        let scheme_host = |origin: &'static str| origin.rsplit_once(':').map(|(sh, _)| sh);
        assert_eq!(
            scheme_host(GESTURE_API_ORIGIN),
            scheme_host(DEMO_APP_ORIGIN),
            "the gesture's API front shares the app origin's scheme and host, so the session cookie \
             is same-site with the page"
        );
        assert_ne!(
            GESTURE_API_ORIGIN, DEMO_APP_ORIGIN,
            "the front has its own port"
        );
        let config = fs::read_to_string(crate::repo_root()?.join("web/playwright.cell.config.ts"))?;
        assert!(
            config.contains("node e2e-cell/tls-proxy.mjs ${API} ${UPSTREAM}")
                && crate::repo_root()?
                    .join("web/e2e-cell/tls-proxy.mjs")
                    .is_file(),
            "the gesture's Playwright run starts the front, from the origins the driver passes"
        );
        assert_eq!(
            LOCAL_ISSUER,
            format!("http://127.0.0.1:{STUB_PORT}"),
            "the stub's issuer is the address its forward listens on, so its Host check passes"
        );
        let package: Value = serde_json::from_str(&fs::read_to_string(
            crate::repo_root()?.join("web/package.json"),
        )?)?;
        let origin = DEMO_APP_ORIGIN
            .strip_prefix("https://")
            .and_then(|o| o.split_once(':'))
            .context("the app origin has a host and a port")?;
        assert_eq!(
            package["scripts"][cell.script].as_str(),
            Some(
                format!(
                    "next dev --experimental-https --hostname {} --port {}",
                    origin.0, origin.1
                )
                .as_str()
            ),
            "web/'s script serves https at the app origin"
        );
        Ok(())
    }

    #[test]
    fn the_gesture_names_each_missing_piece_and_runs_only_with_all_of_them() {
        assert_eq!(gesture_missing(true, true, true, true), None);
        let why = gesture_missing(false, true, false, true).unwrap_or_default();
        assert!(
            why.contains("npm") && why.contains("Chromium") && !why.contains("openssl"),
            "{why}"
        );
        assert!(
            gesture_missing(true, false, true, false)
                .is_some_and(|w| w.contains("node_modules") && w.contains("openssl")),
            "a missing install or openssl is named, never fetched"
        );
    }

    #[test]
    fn the_gesture_result_is_the_recorded_command_or_the_reason_nothing_was_pressed() -> Result<()>
    {
        let recorded = gesture_result(
            r#"{"workspace":"01KDEM0000000000000000000W","agent_id":"agt_1","command_id":"01KDEM0000000000000000000C"}"#,
        )?;
        assert_eq!(
            (
                recorded.workspace.as_str(),
                recorded.agent_id.as_str(),
                recorded.command_id.as_str()
            ),
            (
                "01KDEM0000000000000000000W",
                "agt_1",
                "01KDEM0000000000000000000C"
            )
        );
        let none = gesture_result(r#"{"error":"no agent in the API's workspace to kill"}"#);
        assert!(
            none.is_err_and(|e| e.to_string() == "no agent in the API's workspace to kill"),
            "no agent is the gap, never a fallback"
        );
        assert!(
            gesture_result(r#"{"workspace":"w","agent_id":"a","command_id":""}"#).is_err(),
            "a result with no command id is no record"
        );
        assert!(gesture_result("not json").is_err());
        Ok(())
    }

    #[test]
    fn the_gesture_stays_out_of_web_s_own_e2e_runs() -> Result<()> {
        let web = crate::repo_root()?.join("web");
        let package: Value = serde_json::from_str(&fs::read_to_string(web.join("package.json"))?)?;
        let scripts = package["scripts"].as_object().context("web/'s scripts")?;
        assert!(
            scripts.values().all(|s| !s
                .as_str()
                .unwrap_or_default()
                .contains("playwright.cell.config")),
            "no npm script runs the cell gestures, so web/'s CI jobs never need a cell"
        );
        let config = fs::read_to_string(web.join("playwright.cell.config.ts"))?;
        assert!(config.contains(r#"testDir: "e2e-cell""#));
        assert!(config.contains(&format!(r#"const ORIGIN = "{DEMO_APP_ORIGIN}";"#)));
        Ok(())
    }

    #[test]
    fn a_stopped_group_leaves_no_process_behind_even_one_that_ignores_sigterm() -> Result<()> {
        let mut group = Group::spawn(
            Command::new("sh").args(["-c", "trap '' TERM; sleep 60 & sleep 60 & wait"]),
        )?;
        let (status, _) = group.wait_answering(2, &mut || Ok(()))?;
        assert!(
            status.is_none(),
            "the group is still running before it is stopped"
        );
        assert!(group.alive());
        let leader = group.stop()?;
        assert_eq!(
            leader.and_then(|s| s.signal()),
            Some(9),
            "a leader that ignores SIGTERM is killed, never waited out"
        );
        assert!(
            !group.alive(),
            "no process of the group survives stop, SIGTERM ignored or not"
        );
        Ok(())
    }

    #[test]
    fn dropping_a_group_stops_it() -> Result<()> {
        let group = Group::spawn(Command::new("sh").args(["-c", "sleep 60 & sleep 60"]))?;
        let pgid = group.pgid;
        drop(group);
        assert!(
            !group_alive(pgid),
            "a dropped group leaves no process behind"
        );
        Ok(())
    }
}
