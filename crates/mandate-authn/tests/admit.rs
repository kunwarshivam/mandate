//! The bridge from a session record to `mandate-identity`'s sealed session (identity spec §4.5,
//! §6.4; DEC-652 item 9): only an admitted request gets a session, its kind follows the record's
//! reach, and the roles snapshot is passed through as read.

use std::collections::BTreeMap;

use mandate_authn::{
    EndReason, OrgKind, ProviderAnswer, RefreshSecret, Refreshed, Request, SessionLimits,
    SessionPolicy, SessionRecord, SessionRefusal, SubjectStanding, UtcNanos,
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
    let refreshed = record.refresh(
        &RefreshSecret([1; 32]),
        ProviderAnswer::Granted,
        &RefreshSecret([2; 32]),
        at(3_609),
    );
    assert_eq!(
        refreshed,
        Ok(Refreshed::Rotated {
            access_expires_at: at(3_909)
        }),
        "the admitted request at 10 reset the idle hour, as `authorize` does"
    );
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
    let expired = Err(SessionRefusal::Ended {
        reason: EndReason::Expired,
    });
    let lapsed = idle.admit(Request::Pause, at(3_600), REFERENCE, snapshot());
    assert_eq!(lapsed, expired, "the idle timeout ends the session");
    assert_eq!(idle.ended(), Some(EndReason::Expired));
    for request in [Request::Pause, Request::KillSwitch, Request::Other] {
        let again = idle.admit(request, at(3_601), REFERENCE, snapshot());
        assert_eq!(again, expired, "{request:?} after the lapse");
    }
    let mut fresh = open();
    let late = fresh.admit(Request::Other, at(300), REFERENCE, snapshot());
    assert_eq!(late, Err(SessionRefusal::AccessExpired));
    assert_eq!(fresh.ended(), None, "an expired access token ends nothing");
}

/// Four entries in the order the store read them: an org membership, two workspaces with
/// different roles, one of them with a role still cooling off, and a third workspace.
fn wide_snapshot() -> Vec<Membership> {
    let member = PrincipalId(1);
    let org = OrgId(2);
    let entry = |scope, roles: &[(Role, i64)]| {
        let roles = roles
            .iter()
            .map(|(role, from)| (*role, at(*from)))
            .collect();
        Membership::new(Seal::grant(), member, scope, MembershipState::Active, roles)
    };
    vec![
        entry(Scope::Org(org), &[(Role::OrgAdmin, -DAY_S)]),
        entry(
            Scope::Workspace {
                org,
                workspace: WorkspaceId(7),
            },
            &[(Role::Operator, -60)],
        ),
        entry(
            Scope::Workspace {
                org,
                workspace: WorkspaceId(5),
            },
            &[(Role::Viewer, -60), (Role::Approver, 3_600)],
        ),
        entry(
            Scope::Workspace {
                org,
                workspace: WorkspaceId(9),
            },
            &[(Role::WorkspaceAdmin, -60)],
        ),
    ]
}

const DAY_S: i64 = 86_400;

#[test]
fn every_reach_passes_a_wide_snapshot_through_whole_and_in_order() {
    let mut full = open();
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
    for (name, record, kind) in [
        ("full", &mut full, SessionKind::Full),
        ("route 1", &mut outage, SessionKind::ReductionOnly),
        ("route 2", &mut local, SessionKind::ReductionOnly),
    ] {
        let session = record.admit(Request::Pause, at(299), REFERENCE, wide_snapshot());
        let expected = Session::new(Seal::grant(), REFERENCE, kind, wide_snapshot());
        assert_eq!(session, Ok(expected), "{name}");
        let mut reordered = wide_snapshot();
        reordered.reverse();
        let other = Session::new(Seal::grant(), REFERENCE, kind, reordered);
        assert_ne!(
            record.admit(Request::Pause, at(299), REFERENCE, wide_snapshot()),
            Ok(other),
            "{name}"
        );
    }
}
