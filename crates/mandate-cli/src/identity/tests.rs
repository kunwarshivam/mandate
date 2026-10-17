use super::trusted;

const OWN: u32 = 1000;
const OTHER: u32 = 4242;
const ROOT: u32 = 0;

/// The owner rule, for uids a test process cannot create files as: its own user and root are
/// trusted, any other user is not.
#[test]
fn the_owner_is_this_user_or_root() {
    assert!(trusted(OWN, OWN, 0o644), "a file this user owns");
    assert!(trusted(ROOT, OWN, 0o644), "a file root owns");
    assert!(!trusted(OTHER, OWN, 0o644), "a file another user owns");
    assert!(trusted(ROOT, ROOT, 0o644), "a file root owns, read as root");
    assert!(
        !trusted(OTHER, ROOT, 0o644),
        "a file another user owns, read as root"
    );
}

/// A file its group or others may write is never trusted, whoever owns it.
#[test]
fn group_or_world_write_is_never_trusted() {
    for owner in [OWN, ROOT] {
        assert!(trusted(owner, OWN, 0o600), "owner-only, owned by {owner}");
        assert!(
            !trusted(owner, OWN, 0o664),
            "group-writable, owned by {owner}"
        );
        assert!(
            !trusted(owner, OWN, 0o646),
            "world-writable, owned by {owner}"
        );
    }
}
