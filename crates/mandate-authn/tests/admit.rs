//! The bridge from a session record to `mandate-identity`'s sealed session (identity spec §4.5,
//! §6.4; DEC-652 item 9): only an admitted request gets a session, its kind follows the record's
//! reach, and the roles snapshot is passed through as read.

use std::collections::BTreeMap;

use mandate_authn::{
    EndReason, OrgKind, ProviderAnswer, RefreshSecret, Request, SessionLimits, SessionPolicy,
    SessionRecord, SessionRefusal, SubjectStanding, UtcNanos,
};
use mandate_identity::{
    Membership, MembershipState, OrgId, PrincipalId, Role, Scope, Session, SessionKind, SessionRef,
    WorkspaceId,
};
use mandate_identity_seal::Seal;

const T0: i64 = 1_800_000_000;
const REFERENCE: SessionRef = SessionRef(0x5e55);

fn at(offset_s: i64) -> UtcNanos {
    UtcNanos::from_parts(T0 + offset_s, 0).unwrap()
}

fn open() -> SessionRecord {
    let limits = SessionLimits::resolve(OrgKind::Business, SessionPolicy::default()).unwrap();
    SessionRecord::open(limits, &RefreshSecret([1; 32]), at(0)).unwrap()
}

/// An active operator whose approver role is still cooling off: the snapshot keeps both, each
/// with its effective-from instant, for `authorize` to judge at `now`.
fn snapshot() -> Vec<Membership> {
    let scope = Scope::Workspace {
        org: OrgId(2),
        workspace: WorkspaceId(3),
    };
    let roles = BTreeMap::from([(Role::Operator, at(-60)), (Role::Approver, at(3_600))]);
    vec![Membership::new(
        Seal::grant(),
        PrincipalId(1),
        scope,
        MembershipState::Active,
        roles,
    )]
}

fn expected(kind: SessionKind) -> Session {
    Session::new(Seal::grant(), REFERENCE, kind, snapshot())
}

#[test]
fn a_full_record_admits_a_full_session_with_its_snapshot_as_read() {
    let mut record = open();
    let session = record.admit(Request::Other, at(10), REFERENCE, snapshot());
    assert_eq!(session, Ok(expected(SessionKind::Full)));
}

#[test]
fn an_outage_or_a_local_passkey_admits_a_reduction_only_session() {
    let mut outage = open();
    let answer = ProviderAnswer::Unreachable;
    assert!(
        outage
            .refresh(
                &RefreshSecret([1; 32]),
                answer,
                &RefreshSecret([2; 32]),
                at(300)
            )
            .is_ok()
    );
    let standing = SubjectStanding {
        last_sign_in: Some(at(-60)),
        last_deprovision: None,
    };
    let mut local = SessionRecord::open_reduction_only(standing, at(0)).unwrap();
    for (name, record) in [("route 1", &mut outage), ("route 2", &mut local)] {
        for request in [Request::Pause, Request::KillSwitch] {
            let session = record.admit(request, at(400), REFERENCE, snapshot());
            assert_eq!(
                session,
                Ok(expected(SessionKind::ReductionOnly)),
                "{name} {request:?}"
            );
        }
        let other = record.admit(Request::Other, at(400), REFERENCE, snapshot());
        assert_eq!(other, Err(SessionRefusal::RiskReductionOnly), "{name}");
    }
}

#[test]
fn a_refused_record_builds_no_session() {
    let mut ended = open();
    assert_eq!(
        ended.end(EndReason::Deactivated),
        Some(EndReason::Deactivated)
    );
    let refused = ended.admit(Request::KillSwitch, at(1), REFERENCE, snapshot());
    assert_eq!(
        refused,
        Err(SessionRefusal::Ended {
            reason: EndReason::Deactivated
        })
    );
    let mut idle = open();
    let lapsed = idle.admit(Request::Pause, at(3_600), REFERENCE, snapshot());
    assert_eq!(lapsed, Err(SessionRefusal::IdleExpired));
    let mut fresh = open();
    let late = fresh.admit(Request::Other, at(300), REFERENCE, snapshot());
    assert_eq!(late, Err(SessionRefusal::AccessExpired));
}
