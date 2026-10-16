//! The runnable simulator (DEC-922 item 4): `mandate-rh-sim --script <file> [--port <n>]` serves
//! the script over loopback with the OAuth login required, prints the one endpoint it serves,
//! and takes no other argument, so no option can point it anywhere but loopback.

mod common;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use common::AGENTIC;
use common::wire::Wire;
use serde_json::json;

const BINARY: &str = env!("CARGO_BIN_EXE_mandate-rh-sim");

/// Kills the child when the test ends, however it ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

fn script_file(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(format!("rh-sim-{name}-{}.json", std::process::id()));
    let script = json!({
        "accounts": [{"number": AGENTIC, "agentic_allowed": true, "cash": "750.25",
            "buying_power": "10000", "pattern_day_trader": false}],
        "session": "regular", "quotes": {"SPY": "500"}, "fills": [],
    });
    std::fs::write(&path, script.to_string()).unwrap();
    path
}

#[test]
fn the_binary_serves_the_script_on_loopback_behind_the_login() {
    let script = script_file("serve");
    let child = Command::new(BINARY)
        .args(["--script", script.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut running = Running(child);
    let mut line = String::new();
    let stdout = running.0.stdout.take().unwrap();
    BufReader::new(stdout).read_line(&mut line).unwrap();
    let url = line
        .trim()
        .strip_prefix("mandate-rh-sim: serving ")
        .unwrap();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert!(url.ends_with("/mcp"), "{url}");
    let reply = Wire::new(url).request("initialize", json!({}));
    assert_eq!(reply.status, 401, "the login is required");
    assert!(reply.header("www-authenticate").is_some());
}

#[test]
fn the_binary_refuses_any_other_argument_and_a_bad_script() {
    let script = script_file("refuse");
    let script = script.to_str().unwrap();
    let bad = script_file("bad");
    std::fs::write(&bad, "{\"accounts\": [], \"extra\": 1}").unwrap();
    for args in [
        vec![],
        vec!["--script"],
        vec!["--script", script, "--host", "0.0.0.0"],
        vec!["--script", script, "--port", "not-a-port"],
        vec!["--script", script, "--port", "70000"],
        vec!["--script", script, "--script", script],
        vec!["--script", "/nonexistent/rh-sim-script.json"],
        vec!["--script", bad.to_str().unwrap()],
    ] {
        let output = Command::new(BINARY).args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}
