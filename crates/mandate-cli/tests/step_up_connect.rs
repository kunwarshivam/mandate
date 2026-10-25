//! `mandate step-up confirm-connect` (DEC-2125 item 3; journal spec v0.82 §9.29; E10-13). The
//! journal is `tests/common`'s in-memory `Journal` and the store a map. Every expectation is
//! computed here: the action digest from `oauth_connect_digest` at the founder's disclosure version,
//! written out rather than read from the command, the record's members from §9.29, and whether the
//! journal accepts it from `mandate_journal::Draft::parse`, which judges rule 242. Each refusal is
//! paired with the commit of [`a_connect_confirmation_appends_one_record_and_stores_its_object`],
//! so a command that does nothing passes none of them.

mod common;

use std::collections::BTreeMap;

use common::{CONTROL, Fixture, WORKSPACE, at, body, int, member, member_text, object, text};
use mandate_canon::{Digest, Value};
use mandate_cli::control::ControlError;
use mandate_cli::step_up::{
    CheckedConnect, ConnectConfirmed, ConnectConfirming, EVENT_TYPE, check_connect, confirm_connect,
};
use mandate_connections::manager::oauth_connect_digest;
use mandate_journal::{ArtifactRef, Draft, get_artifact};

const PRINCIPAL: &str = "01J9ZP000000000000000000PR";
const CHALLENGE: &str = "01J9ZC000000000000000000CH";
const SECS: i64 = 1_790_000_100;

/// The connect's digest at the version the founder accepted (DEC-2622), named here, not read from
/// the crate's constants.
fn digest() -> String {
    let d = oauth_connect_digest(WORKSPACE, "alpaca_oauth_token_breadth", "1").unwrap();
    format!("sha256:{d}")
}

fn confirming(code: &str) -> ConnectConfirming<'_> {
    ConnectConfirming {
        workspace: WORKSPACE,
        principal: PRINCIPAL,
        challenge: CHALLENGE,
        code,
    }
}

fn checked(code: &str) -> CheckedConnect {
    check_connect(&confirming(code)).unwrap_or_else(|e| panic!("checked, not: {e}"))
}

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

fn confirm(
    fx: &mut Fixture,
    store: &mut BTreeMap<Digest, Vec<u8>>,
    code: &str,
) -> Result<ConnectConfirmed, ControlError> {
    confirm_connect(&mut fx.journal, store, &checked(code), at(SECS))
}

/// DEC-2125 item 3: exactly one `StepUpConfirmed` of kind `connection` naming the connect's
/// digest, which the journal accepts under rule 242, and the object that digest names, stored.
#[test]
fn a_connect_confirmation_appends_one_record_and_stores_its_object() {
    let mut fx = Fixture::new();
    let mut store = BTreeMap::new();
    let confirmed = confirm(&mut fx, &mut store, "abcd-efgh").unwrap();
    assert!(!confirmed.earlier);
    assert_eq!(confirmed.action_digest, digest());
    let rows = fx.rows(CONTROL);
    assert_eq!(rows.len(), 1, "one record");
    assert_eq!(rows[0].event_id, confirmed.submitted.event_id);
    let parsed = Draft::parse(&rows[0].body).map(|d| d.event_type().to_owned());
    assert_eq!(parsed, Ok(EVENT_TYPE.to_owned()));
    let event = body(&rows[0]);
    for (path, want) in [
        ("environment", "paper"),
        ("stream_id", CONTROL),
        ("actor.kind", "user"),
        ("actor.id", PRINCIPAL),
    ] {
        assert_eq!(member_text(&event, path), Some(want), "{path}");
    }
    let payload = object(&[
        ("challenge", text(CHALLENGE)),
        ("action_kind", text("connection")),
        ("action_digest", text(&digest())),
        ("principal", text(PRINCIPAL)),
        ("code", text("ABCDEFGH")),
        ("confirmed_at", int(SECS)),
    ]);
    assert_eq!(member(&event, "payload"), Some(&payload));
    assert_eq!(
        member(&event, "artifact_refs"),
        Some(&Value::Array(vec![text(&digest())]))
    );
    let reference = ArtifactRef::parse(&digest()).unwrap();
    let stored = get_artifact(&store, &reference).unwrap();
    assert_eq!(
        format!("sha256:{}", Digest::of(&stored)),
        digest(),
        "the stored object is the digest's preimage"
    );
}

/// DEC-2106 item 3, for a connect: a re-run with the same flags finds its own record and appends
/// nothing more.
#[test]
fn a_re_run_finds_its_own_confirmation() {
    let mut fx = Fixture::new();
    let mut store = BTreeMap::new();
    let first = confirm(&mut fx, &mut store, "ABCD-EFGH").unwrap();
    let again = confirm(&mut fx, &mut store, "abcdefgh").unwrap();
    assert!(again.earlier);
    assert_eq!(again.submitted, first.submitted);
    assert_eq!(fx.rows(CONTROL).len(), 1);
}

/// Rule 243: a challenge already confirmed with another code is refused, and nothing is appended.
#[test]
fn a_challenge_confirmed_otherwise_is_refused() {
    let mut fx = Fixture::new();
    let mut store = BTreeMap::new();
    confirm(&mut fx, &mut store, "ABCD-EFGH").unwrap();
    assert_eq!(
        confirm(&mut fx, &mut store, "ABCD-EFGJ").err(),
        Some(refused("challenge_confirmed"))
    );
    assert_eq!(fx.rows(CONTROL).len(), 1, "nothing more");
}

/// A workspace whose control stream is `live` is refused before anything is stored or appended:
/// a connect is confirmed in paper only (DEC-2125 item 3, rule 242).
#[test]
fn a_live_workspace_is_refused_and_nothing_is_stored() {
    let mut fx = Fixture::new();
    fx.append_in("live", CONTROL, "StreamOpened", None, object(&[]));
    let mut store = BTreeMap::new();
    assert_eq!(
        confirm(&mut fx, &mut store, "ABCD-EFGH").err(),
        Some(refused("workspace_environment"))
    );
    assert_eq!(fx.rows(CONTROL).len(), 1, "only the stream's opening");
    assert!(store.is_empty(), "nothing stored");
}

/// The flags, first failing check first, before any journal: a workspace, a principal and a
/// challenge in their forms, and a code as the web app shows it.
#[test]
fn the_flags_are_checked_in_order_before_any_journal() {
    let base = confirming("ABCD-EFGH");
    assert!(check_connect(&base).is_ok());
    let cases = [
        (
            ConnectConfirming {
                workspace: "ws 1",
                ..base
            },
            "owner_workspace_invalid",
        ),
        (
            ConnectConfirming {
                principal: "01j9zp000000000000000000pr",
                ..base
            },
            "principal_invalid",
        ),
        (
            ConnectConfirming {
                challenge: "81J9ZC000000000000000000CH",
                ..base
            },
            "challenge_invalid",
        ),
        (
            ConnectConfirming {
                code: "ABCD-EFGU",
                ..base
            },
            "code_invalid",
        ),
        (
            ConnectConfirming {
                workspace: "",
                principal: "",
                ..base
            },
            "owner_workspace_invalid",
        ),
        (
            ConnectConfirming {
                principal: "x",
                challenge: "y",
                ..base
            },
            "principal_invalid",
        ),
    ];
    for (case, reason) in cases {
        assert_eq!(
            check_connect(&case).err(),
            Some(refused(reason)),
            "{case:?}"
        );
    }
}
