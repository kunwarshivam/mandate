//! The writer's operations (DEC-2120 items 3, 4 and 7).

use secrecy::SecretString;

use super::layout::{
    ALICE, BOB, CAROL, ENDPOINT, Layout, OTHER_WORKSPACE, T0, T1, T2, WORKSPACE, address, entry,
    member, read, workspace,
};
use crate::{AddressError, Deletion, Erasure, Held, Put, UnixNanos, Version, WorkspaceId, Written};

#[test]
#[ignore = "pending E8-14"]
fn put_if_absent_is_bound_to_the_request_digest() {
    let layout = Layout::new();
    let writer = layout.writer();
    let ws = workspace(WORKSPACE);
    let one = address(1);
    assert_eq!(
        writer.put_if_absent(&ws, &one, &entry(ALICE, ENDPOINT, 1), T0),
        Ok(Put::Written(Version(1)))
    );
    assert_eq!(
        writer.put_if_absent(&ws, &one, &entry(ALICE, ENDPOINT, 1), T1),
        Ok(Put::AlreadyHeld(Version(1))),
        "the same request again writes nothing"
    );
    for (who, digest) in [(ALICE, 2), (BOB, 1)] {
        assert_eq!(
            writer.put_if_absent(&ws, &one, &entry(who, ENDPOINT, digest), T1),
            Err(AddressError::Conflict {
                address_ref: one.clone()
            }),
            "another digest or member under one address_ref is idempotency_conflict"
        );
    }
    assert_eq!(
        writer.inert_older_than(&ws, T2, T2),
        Ok(vec![Written {
            address_ref: one,
            version: Version(1),
            written_at: T0
        }]),
        "neither the repeat nor a conflict wrote"
    );
}

#[test]
#[ignore = "pending E8-14"]
fn every_write_bumps_the_version() {
    let layout = Layout::new();
    let writer = layout.writer();
    let ws = workspace(WORKSPACE);
    let one = address(1);
    let it = entry(ALICE, ENDPOINT, 1);
    assert_eq!(
        writer.put_if_absent(&ws, &one, &it, T0),
        Ok(Put::Written(Version(1)))
    );
    assert_eq!(writer.put_again(&ws, &one, &it, T1), Ok(Version(2)));
    assert_eq!(writer.put_again(&ws, &one, &it, T2), Ok(Version(3)));
    assert_eq!(
        writer.put_again(&ws, &one, &entry(BOB, ENDPOINT, 1), T2),
        Err(AddressError::Conflict {
            address_ref: one.clone()
        })
    );
    assert_eq!(
        writer.inert_older_than(&ws, UnixNanos(T2.0 + 1), T2),
        Ok(vec![Written {
            address_ref: one.clone(),
            version: Version(3),
            written_at: T2
        }])
    );
    assert_eq!(
        writer.inert_older_than(&ws, T2, T2),
        Ok(vec![]),
        "written_at moved with the write"
    );
    assert_eq!(
        writer.compare_and_delete(&ws, &one, Version(3)),
        Ok(Deletion::Deleted)
    );
    assert_eq!(
        writer.put_again(&ws, &one, &it, T2),
        Ok(Version(1)),
        "put_again recreates it"
    );
}

#[test]
#[ignore = "pending E8-14"]
fn compare_and_delete_fails_on_a_stale_version_and_erase_does_not() {
    let layout = Layout::new();
    let writer = layout.writer();
    let reader = layout.reader();
    let ws = workspace(WORKSPACE);
    let it = entry(ALICE, ENDPOINT, 1);
    for n in [1, 2] {
        writer.put_if_absent(&ws, &address(n), &it, T0).unwrap();
        writer.put_again(&ws, &address(n), &it, T1).unwrap();
    }
    assert_eq!(
        writer.compare_and_delete(&ws, &address(1), Version(1)),
        Ok(Deletion::Stale)
    );
    assert_eq!(
        read(&reader, ALICE, 1),
        Ok(true),
        "a stale delete leaves the entry"
    );
    assert_eq!(
        writer.compare_and_delete(&ws, &address(1), Version(2)),
        Ok(Deletion::Deleted)
    );
    assert_eq!(
        writer.compare_and_delete(&ws, &address(1), Version(2)),
        Ok(Deletion::Absent)
    );
    assert_eq!(read(&reader, ALICE, 1), Ok(false));
    assert_eq!(
        writer.erase(&ws, &address(2)),
        Ok(Erasure::Erased),
        "whatever its version"
    );
    assert_eq!(writer.erase(&ws, &address(2)), Ok(Erasure::Absent));
    assert_eq!(read(&reader, ALICE, 2), Ok(false));
}

#[test]
#[ignore = "pending E8-14"]
fn purge_workspace_removes_that_workspace_only() {
    let layout = Layout::new();
    let writer = layout.writer();
    let reader = layout.reader();
    let (ws, other) = (workspace(WORKSPACE), workspace(OTHER_WORKSPACE));
    for w in [&ws, &other] {
        writer
            .put_if_absent(w, &address(1), &entry(ALICE, ENDPOINT, 1), T0)
            .unwrap();
    }
    assert_eq!(writer.purge_workspace(&ws), Ok(()));
    assert!(!layout.workspace_dir(WORKSPACE).exists());
    assert_eq!(read(&reader, ALICE, 1), Ok(false));
    assert!(matches!(
        reader.entry(&other, &member(ALICE), &address(1)),
        Ok(Some(_))
    ));
    assert_eq!(
        writer.purge_workspace(&ws),
        Ok(()),
        "purging an absent workspace is done already"
    );
}

#[test]
#[ignore = "pending E8-14"]
fn entries_holding_finds_only_that_members_entries() {
    let layout = Layout::new();
    let writer = layout.writer();
    let (ws, other) = (workspace(WORKSPACE), workspace(OTHER_WORKSPACE));
    let shared = "https://push.example/shared";
    let puts = [
        (&ws, 1, ALICE, shared),
        (&ws, 2, ALICE, "https://push.example/alice-only"),
        (&ws, 3, ALICE, shared),
        (&ws, 4, BOB, shared),
        (&other, 5, ALICE, shared),
    ];
    for (w, n, who, endpoint) in puts {
        writer
            .put_if_absent(w, &address(n), &entry(who, endpoint, n), T0)
            .unwrap();
    }
    let holding = |w: &WorkspaceId, who: &str, endpoint: &str| {
        writer.entries_holding(w, &member(who), &SecretString::from(endpoint.to_owned()))
    };
    let held = |n: u8| Held {
        address_ref: address(n),
        version: Version(1),
    };
    assert_eq!(holding(&ws, ALICE, shared), Ok(vec![held(1), held(3)]));
    assert_eq!(holding(&ws, BOB, shared), Ok(vec![held(4)]));
    assert_eq!(holding(&ws, CAROL, shared), Ok(vec![]));
    assert_eq!(
        holding(&ws, BOB, "https://push.example/alice-only"),
        Ok(vec![])
    );
    assert_eq!(holding(&other, ALICE, shared), Ok(vec![held(5)]));
    assert_eq!(
        holding(&ws, ALICE, "https://push.example/SHARED"),
        Ok(vec![])
    );
}
