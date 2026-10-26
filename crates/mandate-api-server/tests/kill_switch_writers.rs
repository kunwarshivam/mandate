//! DEC-2517 item 2 (E8-9k, notifications spec §3.4): every writer of a user's kill switch commits
//! the `OwnerAlertSent` naming its `OwnerCommandIssued` in the same batch, so a user's switch is one
//! notice whichever surface issued it, and the executor, which alerts no user's switch, leaves none
//! silent. Every non-test source file of the workspace that names both `OwnerCommandIssued` and
//! `kill_switch` is listed here, as a writer with the test that pins its alert or as a reader, so a
//! writer added later fails this test until it is listed, and listing it as a writer names the test
//! that proves its alert.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What a file that names a kill switch command does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Appends the command, and with it the alert, as the named test pins.
    Writes(&'static str),
    /// Reads the command or names it, and appends neither.
    Reads,
}

/// Every such file, by its path under `crates/`.
const LISTED: [(&str, Role); 11] = [
    (
        "mandate-api-server/src/commands.rs",
        Role::Writes(
            "mandate-api-server writes_http::a_kill_switch_commits_one_owner_alert_naming_its_command_in_its_batch",
        ),
    ),
    (
        "mandate-api-server/src/writes.rs",
        Role::Writes(
            "mandate-api-server writes_http::a_kill_switch_commits_one_owner_alert_naming_its_command_in_its_batch",
        ),
    ),
    (
        "mandate-cli/src/agent.rs",
        Role::Writes("mandate-cli brake::a_kill_switch_commits_its_owner_alert_in_its_batch"),
    ),
    (
        "mandate-cli/src/control.rs",
        Role::Writes("mandate-cli brake::a_kill_switch_commits_its_owner_alert_in_its_batch"),
    ),
    ("mandate-deploy-manager/src/control.rs", Role::Reads),
    ("mandate-executor/src/kill.rs", Role::Reads),
    ("mandate-executord/src/follow.rs", Role::Reads),
    ("mandate-journal/src/workspace.rs", Role::Reads),
    (
        "mandate-refcases/src/mandate/escalation/lifecycle.rs",
        Role::Reads,
    ),
    ("mandate-runtime/src/payload.rs", Role::Reads),
    ("mandate-runtime/src/escalation.rs", Role::Reads),
];

/// Every `.rs` file under `dir` that is not a test: no `tests` directory and no `tests.rs`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "tests" && name != "target" {
                sources(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") && name != "tests.rs" {
            out.push(path);
        }
    }
}

#[test]
fn every_kill_switch_command_writer_is_listed_with_the_test_that_pins_its_alert() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    sources(&crates, &mut files);
    let named: Vec<String> = files
        .iter()
        .filter(|path| path.components().any(|c| c.as_os_str() == "src"))
        .filter_map(|path| {
            let source = std::fs::read_to_string(path).ok()?;
            (source.contains("OwnerCommandIssued") && source.contains("\"kill_switch\""))
                .then(|| path.strip_prefix(&crates).ok()?.to_str().map(str::to_owned))
                .flatten()
        })
        .collect();
    let listed: BTreeMap<&str, Role> = LISTED.into_iter().collect();
    let unlisted: Vec<&String> = named
        .iter()
        .filter(|path| !listed.contains_key(path.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "a file names a kill switch command and is not listed; a writer commits the command's \
         OwnerAlertSent in its batch (DEC-2517 item 1) and is listed with the test that pins it: \
         {unlisted:?}"
    );
    let gone: Vec<&&str> = listed
        .keys()
        .filter(|path| !named.iter().any(|n| n == *path))
        .collect();
    assert!(gone.is_empty(), "listed but no longer naming one: {gone:?}");
    for role in listed.values() {
        if let Role::Writes(pinned) = role {
            assert!(
                pinning_test_exists(&crates, pinned),
                "no such test: {pinned}"
            );
        }
    }
}

/// Whether `pinned`, `<crate> <file>::<test>`, names a test function in `crates/<crate>/tests/`.
fn pinning_test_exists(crates: &Path, pinned: &str) -> bool {
    let Some((krate, test)) = pinned.split_once(' ') else {
        return false;
    };
    let Some((file, function)) = test.split_once("::") else {
        return false;
    };
    let path = crates.join(krate).join("tests").join(format!("{file}.rs"));
    std::fs::read_to_string(path).is_ok_and(|source| source.contains(&format!("fn {function}()")))
}
