//! `mandate disclosure accept` (DEC-2134; journal spec §9.2 `DisclosureAccepted`, V-005): the
//! go-live disclosure's version is the SHA-256 of its file's canonical JSON, its code is DEC-530
//! item 4's over the gesture, a wrong code writes nothing, and a right one stores the document and
//! commits one `DisclosureAccepted` with `cli_confirm` step-up that the journal's own validator
//! admits. Every expectation is computed here, not by the module.

mod common;

use std::collections::BTreeMap;

use common::{CONTROL, FixedIds, Journal, at, owner, stream};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::control::ControlJournal;
use mandate_cli::disclosure::{accept, code, document};
use mandate_journal::Draft;

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_792_000_000;
const FILE: &str = include_str!("../../../fixtures/disclosures/go_live_risk.json");
const PENDING_COUNSEL: &str = "Pending counsel — not for real-money use";
/// The go-live disclosure's version as `web/` and `cargo xtask cell journey` compute it, each by its
/// own canonical JSON; a changed word changes all three together.
const PINNED_VERSION: &str =
    "sha256:35ab1cb9c3861954a3d846866198e8b4c801532de56e8b00ee86e51404d541f2";

fn version() -> String {
    let canonical = to_canonical(&parse(FILE.as_bytes()).unwrap());
    format!("sha256:{}", Digest::of(&canonical).to_hex())
}

fn gesture_code(version: &str) -> String {
    let gesture = format!(
        r#"{{"document":"go_live_risk","gesture":"disclosure_accept","version":"{version}"}}"#
    );
    let canonical = to_canonical(&parse(gesture.as_bytes()).unwrap());
    Digest::of(&canonical).to_hex()[..8].to_owned()
}

#[test]
fn the_go_live_disclosure_is_a_placeholder_pending_counsel_with_its_own_version() {
    let doc = document("go_live_risk").unwrap();
    assert_eq!(doc.version, version());
    assert_eq!(
        doc.version, PINNED_VERSION,
        "web/ and xtask pin the same version"
    );
    assert_eq!(code(&doc).unwrap(), gesture_code(&version()));
    let parsed = parse(&doc.canonical).unwrap();
    assert_eq!(
        parsed.get("status").and_then(Value::as_str),
        Some(PENDING_COUNSEL)
    );
    let sections = parsed.get("sections").and_then(Value::as_array).unwrap();
    assert_eq!(sections.len(), 4);
    for section in sections {
        for paragraph in section.get("paragraphs").and_then(Value::as_array).unwrap() {
            assert!(
                paragraph.as_str().unwrap().starts_with("Placeholder."),
                "{paragraph:?}"
            );
        }
    }
    for unknown in ["", "go_live", "leveraged_etp", "GO_LIVE_RISK"] {
        let refused = document(unknown).unwrap_err().to_string();
        assert!(refused.contains("document_unknown"), "{unknown}: {refused}");
    }
}

#[test]
fn a_wrong_code_writes_nothing_and_mints_no_assertion() {
    let (mut journal, mut store, mut ids) = (Journal::default(), Store::new(), FixedIds::default());
    let doc = document("go_live_risk").unwrap();
    for wrong in ["", "00000000", &gesture_code("sha256:0000")] {
        let refused = accept(
            &mut journal,
            &mut store,
            &mut ids,
            &owner(),
            (&doc, wrong),
            at(NOW),
        );
        let why = refused.unwrap_err().to_string();
        assert!(why.contains("code_mismatch"), "{why}");
    }
    assert!(journal.attempts.is_empty(), "nothing appended");
    assert!(store.is_empty(), "nothing stored");
    assert_eq!(ids.assertions, 0, "no assertion minted");
}

#[test]
fn the_right_code_stores_the_document_and_commits_a_valid_acceptance_with_step_up() {
    let (mut journal, mut store, mut ids) = (Journal::default(), Store::new(), FixedIds::default());
    let doc = document("go_live_risk").unwrap();
    let typed = gesture_code(&version());
    let submitted = accept(
        &mut journal,
        &mut store,
        &mut ids,
        &owner(),
        (&doc, &typed),
        at(NOW),
    )
    .unwrap();
    assert_eq!(
        store.get(&Digest::of(&doc.canonical)),
        Some(&doc.canonical),
        "the accepted text is stored under its version"
    );
    let rows = journal.rows(&stream(CONTROL)).unwrap();
    let row = rows
        .iter()
        .find(|r| r.event_id == submitted.event_id)
        .unwrap();
    let parsed = Draft::parse(&row.body).expect("the journal's validator admits the acceptance");
    assert_eq!(parsed.event_type(), "DisclosureAccepted");
    let body = parse(&row.body).unwrap();
    let member = |path: &[&str]| {
        path.iter()
            .try_fold(&body, |v, k| v.get(k))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    assert_eq!(
        member(&["payload", "document"]).as_deref(),
        Some("go_live_risk")
    );
    assert_eq!(member(&["payload", "version"]), Some(version()));
    assert_eq!(member(&["payload", "user"]), Some(owner().user));
    assert_eq!(
        member(&["payload", "step_up", "method"]).as_deref(),
        Some("cli_confirm")
    );
    assert_eq!(
        member(&["payload", "step_up", "assertion_id"]).as_deref(),
        Some("cli-assertion-1")
    );
    assert_eq!(
        member(&["payload", "step_up", "authenticated_at"]).as_deref(),
        Some("2026-10-14T17:46:40.000000000Z")
    );
    assert_eq!(member(&["actor", "kind"]).as_deref(), Some("user"));
    assert_eq!(member(&["actor", "id"]), Some(owner().user));
    let refs: Vec<String> = body
        .get("artifact_refs")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    assert_eq!(refs, [version()]);
    assert_eq!(ids.assertions, 1);
}

/// The binary without `--code` prints the document's version and its code and opens no journal: a
/// DSN that names no host still exits 0. With a wrong code it opens the journal only after the
/// owner flags pass, so an unreachable one refuses, and nothing names the DSN.
#[test]
fn the_binary_shows_the_code_without_a_journal_and_never_prints_the_dsn() {
    let dsn = "postgres://gl-user-sentinel@gl-host-sentinel.invalid:1/gl-db";
    let run = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_mandate"))
            .args([
                "disclosure",
                "accept",
                "go_live_risk",
                "--workspace",
                "ws1",
                "--user",
                "u1",
            ])
            .args(["--journal", dsn, "--store", "/nonexistent-gl-store"])
            .args(extra)
            .output()
            .unwrap()
    };
    let shown = run(&[]);
    assert!(shown.status.success(), "{shown:?}");
    let out = String::from_utf8(shown.stdout).unwrap();
    assert_eq!(
        out,
        format!(
            "document go_live_risk version {}\ncode {}\n",
            version(),
            gesture_code(&version())
        )
    );
    let refused = run(&["--code", &gesture_code(&version())]);
    assert!(!refused.status.success());
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!all.contains("sentinel"), "{all}");
    let unknown = std::process::Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args([
            "disclosure",
            "accept",
            "leveraged_etp",
            "--workspace",
            "ws1",
            "--user",
            "u1",
            "--journal",
            dsn,
            "--store",
            "/x",
        ])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("document_unknown"));
}
