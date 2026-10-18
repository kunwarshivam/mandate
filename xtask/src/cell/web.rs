//! How `cargo xtask cell demo` runs `web/` (lane W2, DEC-2406): on its own fixtures, or against the
//! cell's API as the demo owner once sign-in through the stub issuer is ready. The scenario's web
//! gestures (`scenario.rs`, lane E2E) call [`kill_switch_from_web`], which starts its own `web/`
//! and drives it with Playwright (`web/e2e-cell/`).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use super::{DEMO_APP_ORIGIN, DEMO_SUBJECT, LOCAL_ISSUER, WEB_URL, state};

/// The local API as `web/` reaches it: `localhost`, so its `SameSite=Strict` cookie is same-site
/// with the app origin [`DEMO_APP_ORIGIN`] (DEC-2406).
const WEB_API_ORIGIN: &str = "http://localhost:8787";

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

/// Step 6 from `web/`, in `workspace`, the one the cell's API hosts (the scenario passes its own run's,
/// never a fixed id), and to `agent` when it names one (else the first listed agent not stopped; a
/// named agent already stopped is an `Err`, since D10 offers nothing to confirm for it): starts
/// `next dev` against the cell's API as the demo owner, signs in through
/// the stub, opens Stop and presses one listed agent's kill switch, and returns what the API
/// recorded. An `Err` is the gap the scenario prints: a missing tool, no agent to kill, or a run
/// that failed or ran past [`GESTURE_POLLS`] (its log is named).
pub fn kill_switch_from_web(workspace: &str, agent: Option<&str>) -> Result<WebGesture> {
    if let Some(why) = gesture_missing(
        answers("npm", "--version"),
        Path::new("web/node_modules").exists(),
        chromium_cached(),
        answers("openssl", "version"),
    ) {
        bail!(why);
    }
    let dir = state()?.join("web-gesture");
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
    let mut child = Command::new("npx")
        .current_dir("web")
        .args(["playwright", "test", "-c", "playwright.cell.config.ts"])
        .envs(
            web_run(true, workspace)
                .env
                .iter()
                .map(|(n, v)| (*n, v.as_str())),
        )
        .env("OWLHEAD_CELL_TLS_KEY", &key)
        .env("OWLHEAD_CELL_TLS_CERT", &cert)
        .env("OWLHEAD_CELL_RESULT", &result)
        .env("OWLHEAD_CELL_AGENT", agent.unwrap_or_default())
        .stdout(output.try_clone()?)
        .stderr(output)
        .spawn()
        .context("starting web/'s Playwright gesture")?;
    let mut polls = 0;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if polls >= GESTURE_POLLS {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "web/'s gesture ran past {}s and was stopped; its log is {}",
                (POLL * GESTURE_POLLS).as_secs(),
                log.display()
            );
        }
        sleep(POLL);
        polls += 1;
    };
    eprintln!(
        "    web/: the gesture's Playwright run took about {:.1}s",
        (POLL * polls).as_secs_f64()
    );
    let text = fs::read_to_string(&result).ok();
    match (status.success(), text) {
        (_, Some(text)) if text.contains("\"error\"") => gesture_result(&text),
        (true, Some(text)) => gesture_result(&text),
        _ => bail!(
            "web/'s gesture failed ({status}); its log is {}",
            log.display()
        ),
    }
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

    use super::super::{API_LOCAL_PORT, DEMO_WORKSPACE, STUB_PORT};
    use super::*;

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
}
