//! The binary itself (DEC-2326, E21-62): a refused start exits 2 with its code alone on stderr,
//! and a probe of a process that is not running fails.

use std::process::{Command, Output};

fn gatewayd(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mandate-gatewayd"))
        .args(args)
        .env_clear()
        .output()
        .unwrap()
}

fn refused(output: &Output) -> (Option<i32>, String) {
    (
        output.status.code(),
        String::from_utf8(output.stderr.clone()).unwrap(),
    )
}

/// DEC-2216 item 4, DEC-2326 item 3: a live start missing its vault flags exits 2 with the first
/// one's code, before it reads anything, builds a provider client, or opens a socket.
#[test]
fn a_live_start_without_its_vault_flags_exits_with_its_code() {
    let output = gatewayd(&[
        "run",
        "--registry",
        "/r.json",
        "--listen",
        "127.0.0.1:0",
        "--probe",
        "/tmp/unused.sock",
    ]);
    assert_eq!(
        refused(&output),
        (Some(2), "vault_address_missing\n".to_owned())
    );
}

/// DEC-2326 item 2: with its start-up values unset, `run` exits 2 naming the first one missing,
/// before it reads its recording or opens a listener.
#[test]
fn a_run_without_its_values_exits_with_the_first_one_s_code() {
    let dir = std::env::temp_dir().join(format!("gatewayd-bin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("process.sock");
    let output = gatewayd(&[
        "run",
        "--recorded",
        "/no/such/research.json",
        "--listen",
        "127.0.0.1:0",
        "--probe",
        socket.to_str().unwrap(),
    ]);
    assert_eq!(
        refused(&output),
        (Some(2), "workspace_missing\n".to_owned())
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// DEC-994 item 5: a probe of a socket no process serves fails, exit 1.
#[test]
fn a_probe_of_no_process_fails() {
    let output = gatewayd(&["probe", "livez", "--probe", "/tmp/no-gatewayd-here.sock"]);
    assert_eq!(output.status.code(), Some(1));
}

/// Arguments that start nothing exit 2 with their code.
#[test]
fn arguments_that_start_nothing_exit_with_their_code() {
    let output = gatewayd(&["serve", "--probe", "/tmp/p.sock"]);
    assert_eq!(refused(&output), (Some(2), "usage\n".to_owned()));
}
