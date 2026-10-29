//! What route 2's judge reads besides the assertion (§4.10 route 2 item 2, E9-1 S2b-2): the
//! control stream's passkey records (journal spec §9.31), read as the membership parser reads its
//! rows, and DEC-2319 item 2's interim standing, until E9-13: the last sign-in this process saw, and
//! no membership record naming the member since.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::at;
use common::control::{OTHER_WORKSPACE, Rows, T0, member, other_stream, record};
use common::hosted;
use mandate_api_server::members::{RowsRefused, credential_records};
use mandate_api_server::standing::{Standings, interim_refuses};
use mandate_authn::SubjectStanding;
use mandate_canon::encode_ulid;
use mandate_identity::{InvitationId, MembershipEvent, PrincipalId, Role};
use mandate_passkey::credentials::{Assertion, CredentialRecord, Enrolment, PasskeyAlg, Removal};
use serde_json::{Value as Json, json};

const ALICE: PrincipalId = PrincipalId(7);
const BOB: PrincipalId = PrincipalId(8);
const CREDENTIAL: &str = "AAECAwQFBgcICQoLDA0ODw";
const CHALLENGE: &str = "01JAA3P0A000000000000000H1";
const ASSERTION: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";
const KEY: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";

fn enrolled(member: PrincipalId, credential: &str) -> Json {
    json!({
        "member": encode_ulid(member.0), "kind": "passkey", "credential": credential,
        "public_key": KEY, "alg": "es256", "sign_count": 3, "cool_off_ends": null,
        "step_up": null,
    })
}

fn removed(member: PrincipalId, credential: &str) -> Json {
    json!({
        "member": encode_ulid(member.0), "credential": credential,
        "step_up": {"assertion_id": "01JAA3P4A000000000000000S1", "authenticated_at": "2026-10-19T15:00:05.250000000Z", "method": "passkey"},
    })
}

fn enrolment(seq: u64, member: PrincipalId, credential: &str) -> CredentialRecord {
    CredentialRecord::Enrolled(Enrolment {
        seq,
        member,
        credential: credential.to_owned(),
        public_key: KEY.to_owned(),
        alg: PasskeyAlg::Es256,
        sign_count: 3,
        cool_off_ends: None,
        backup_eligible: None,
    })
}

fn parsed(rows: &Rows) -> Result<Vec<CredentialRecord>, RowsRefused> {
    credential_records(hosted().workspace, &rows.rows)
}

#[test]
fn the_hosted_streams_passkey_records_are_read_in_order_and_every_other_row_passed_over() {
    let mut rows = Rows::opened();
    rows.hosted("CredentialEnrolled", T0 - 100, enrolled(ALICE, CREDENTIAL));
    rows.hosted("MemberDeactivated", T0 - 90, member(BOB));
    rows.push(
        &other_stream(),
        "live",
        "CredentialEnrolled",
        T0 - 80,
        enrolled(BOB, "EBESExQVFhcYGRobHB0eHw"),
    );
    rows.edited("StepUpAsserted", T0 - 70, |e| {
        e["causation_id"] = json!(encode_ulid(1));
        e["actor"] =
            json!({"kind": "user", "id": encode_ulid(ALICE.0), "version": "1", "build": null});
        e["payload"] = json!({
            "challenge": CHALLENGE, "credential": CREDENTIAL, "sign_count": 4,
            "assertion": ASSERTION,
        });
    });
    rows.hosted("CredentialRemoved", T0 - 60, removed(ALICE, CREDENTIAL));
    assert_eq!(
        parsed(&rows),
        Ok(vec![
            enrolment(2, ALICE, CREDENTIAL),
            CredentialRecord::Asserted(Assertion {
                seq: 5,
                member: ALICE,
                credential: CREDENTIAL.to_owned(),
                challenge: CHALLENGE.to_owned(),
                sign_count: 4,
                event_time: at(T0 - 70),
            }),
            CredentialRecord::Removed(Removal {
                seq: 6,
                member: ALICE,
                credential: CREDENTIAL.to_owned(),
            }),
        ]),
        "another workspace's stream is not the hosted one: {OTHER_WORKSPACE}"
    );
}

#[test]
fn an_unreadable_record_another_environment_or_an_unopened_stream_refuses_every_record() {
    let mut bad_id = Rows::opened();
    bad_id.hosted("CredentialEnrolled", T0, enrolled(ALICE, "not base64url!"));
    assert_eq!(parsed(&bad_id), Err(RowsRefused::Unreadable { seq: 2 }));
    let mut version = Rows::opened();
    version.edited("CredentialRemoved", T0, |e| {
        e["schema_version"] = json!(2);
        e["payload"] = removed(ALICE, CREDENTIAL);
    });
    assert_eq!(parsed(&version), Err(RowsRefused::Unreadable { seq: 2 }));
    let mut elsewhere = Rows::opened();
    elsewhere.hosted("CredentialEnrolled", T0 - 10, enrolled(ALICE, CREDENTIAL));
    elsewhere.push(
        &common::control::hosted_stream(),
        "live",
        "CredentialEnrolled",
        T0,
        enrolled(BOB, "EBESExQVFhcYGRobHB0eHw"),
    );
    assert_eq!(
        parsed(&elsewhere),
        Err(RowsRefused::OtherEnvironment { seq: 3 })
    );
    let mut membership_elsewhere = Rows::opened();
    membership_elsewhere.hosted("MemberDeactivated", T0 - 10, member(BOB));
    membership_elsewhere.push(
        &common::control::hosted_stream(),
        "live",
        "MemberRemoved",
        T0,
        member(BOB),
    );
    assert_eq!(
        parsed(&membership_elsewhere),
        Ok(Vec::new()),
        "only a passkey record's environment is the parser's to judge"
    );
    let mut unopened = Rows { rows: Vec::new() };
    unopened.hosted("CredentialEnrolled", T0, enrolled(ALICE, CREDENTIAL));
    assert_eq!(parsed(&unopened), Err(RowsRefused::NotOpened));
    let mut garbled = Rows::opened();
    garbled.hosted("CredentialEnrolled", T0, enrolled(ALICE, CREDENTIAL));
    garbled.rows[1].body = b"{".to_vec();
    assert_eq!(parsed(&garbled), Err(RowsRefused::Unreadable { seq: 2 }));
}

#[test]
fn the_last_sign_in_is_kept_per_member_and_never_moves_back() {
    let standings = Standings::new();
    let none = SubjectStanding::default();
    assert_eq!(standings.standing(ALICE), none, "none seen at start");
    standings.signed_in(ALICE, at(T0));
    standings.signed_in(ALICE, at(T0 - 5));
    assert_eq!(
        standings.standing(ALICE),
        SubjectStanding {
            last_sign_in: Some(at(T0)),
            last_deprovision: None,
        },
        "an earlier sign-in arriving late moves nothing back"
    );
    standings.signed_in(ALICE, at(T0 + 5));
    assert_eq!(standings.standing(ALICE).last_sign_in, Some(at(T0 + 5)));
    assert_eq!(standings.standing(BOB), none, "each member their own");
    standings.signed_in(BOB, at(T0 - 50));
    assert_eq!(standings.standing(BOB).last_sign_in, Some(at(T0 - 50)));
    assert_eq!(standings.standing(ALICE).last_sign_in, Some(at(T0 + 5)));
    assert_eq!(
        Standings::new().standing(ALICE),
        none,
        "a new process, as after a restart, holds none"
    );
}

#[test]
fn route_two_is_refused_without_a_sign_in_or_after_a_membership_record_naming_the_member() {
    let signed_in = |secs: i64| SubjectStanding {
        last_sign_in: Some(at(secs)),
        last_deprovision: None,
    };
    let roles = BTreeSet::from([Role::Operator]);
    let activated = record(
        2,
        T0 - 100,
        MembershipEvent::Activated {
            member: ALICE,
            invitation: None,
            roles: roles.clone(),
            independent_approval_required: false,
            cool_off_ends_at: at(T0 - 100),
        },
    );
    let before = [activated];
    assert!(!interim_refuses(signed_in(T0 - 50), &before, ALICE));
    assert!(
        interim_refuses(SubjectStanding::default(), &before, ALICE),
        "no sign-in seen"
    );
    assert!(interim_refuses(SubjectStanding::default(), &[], ALICE));
    let naming = [
        MembershipEvent::Activated {
            member: ALICE,
            invitation: None,
            roles: roles.clone(),
            independent_approval_required: false,
            cool_off_ends_at: at(T0),
        },
        MembershipEvent::RoleChanged {
            member: ALICE,
            added: BTreeMap::new(),
            removed: roles.clone(),
            independent_approval_required: false,
        },
        MembershipEvent::Deactivated { member: ALICE },
        MembershipEvent::Reactivated {
            member: ALICE,
            roles: roles.clone(),
            independent_approval_required: false,
            cool_off_ends_at: at(T0),
        },
        MembershipEvent::Removed { member: ALICE },
    ];
    for event in naming {
        for (secs, refused) in [(T0 - 51, false), (T0 - 50, true), (T0 - 10, true)] {
            let records = [before[0].clone(), record(3, secs, event.clone())];
            assert_eq!(
                interim_refuses(signed_in(T0 - 50), &records, ALICE),
                refused,
                "{event:?} at {secs}"
            );
            assert!(
                !interim_refuses(signed_in(T0 - 50), &records, BOB),
                "another member's record: {event:?}"
            );
        }
    }
    let unnamed = [
        before[0].clone(),
        record(
            3,
            T0 - 10,
            MembershipEvent::Invited {
                invitation: InvitationId(9),
                roles,
                expires_at: at(T0 + 100),
            },
        ),
        record(
            4,
            T0 - 5,
            MembershipEvent::InvitationRevoked {
                invitation: InvitationId(9),
            },
        ),
        record(5, T0 - 1, MembershipEvent::Deactivated { member: BOB }),
    ];
    assert!(!interim_refuses(signed_in(T0 - 50), &unnamed, ALICE));
    let refused = SubjectStanding {
        last_sign_in: Some(at(T0 - 50)),
        last_deprovision: Some(at(T0 - 40)),
    };
    assert!(
        !interim_refuses(refused, &before, ALICE),
        "a deprovision is the session record's to judge, not the interim's"
    );
}
