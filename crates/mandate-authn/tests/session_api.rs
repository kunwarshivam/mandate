//! What the session API prints and journals (identity spec §12.1, ID-9; backlog E9-1, slice A2):
//! a refresh secret's `Debug`, each `SessionRevoked` reason code, and a record that keeps only a
//! digest of its refresh token.

use mandate_authn::{
    EndReason, OrgKind, RefreshSecret, SessionLimits, SessionPolicy, SessionRecord, UtcNanos,
};

#[test]
#[ignore = "pending E9-1"]
fn a_session_keeps_only_a_digest_of_its_refresh_token() {
    let limits = SessionLimits::resolve(OrgKind::Business, SessionPolicy::default()).unwrap();
    let now = UtcNanos::from_parts(1_800_000_000, 0).unwrap();
    let s = SessionRecord::open(limits, &RefreshSecret([1; 32]), now).unwrap();
    let mut sha256_of_token = [0u8; 32];
    sha256_of_token.copy_from_slice(ring::digest::digest(&ring::digest::SHA256, &[1; 32]).as_ref());
    assert_eq!(s.refresh_digest(), Some(sha256_of_token));
    assert_ne!(s.refresh_digest(), Some([1; 32]));
    assert_eq!(s.rotated_digests(), Vec::<[u8; 32]>::new());
    let shown = format!("{s:?}");
    assert!(!shown.contains(&format!("{:?}", [1u8; 32])), "{shown}");
    assert!(!shown.contains(&"01".repeat(8)), "{shown}");
}

#[test]
fn a_refresh_secret_prints_nothing_and_each_end_reason_has_its_spec_code() {
    assert_eq!(format!("{:?}", RefreshSecret([7; 32])), "RefreshSecret(..)");
    let codes = [
        EndReason::SignOut,
        EndReason::Deactivated,
        EndReason::Deprovisioned,
        EndReason::RefreshFailed,
        EndReason::RefreshReuse,
        EndReason::Admin,
    ]
    .map(EndReason::code);
    assert_eq!(
        codes,
        [
            "sign_out",
            "deactivated",
            "deprovisioned",
            "refresh_failed",
            "refresh_reuse",
            "admin"
        ]
    );
}
