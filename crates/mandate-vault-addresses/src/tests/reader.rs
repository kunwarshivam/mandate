//! The reader, the files it refuses, the sweep's listing, rotation, and ids (DEC-2120 items 2,
//! 3, 6 and 7).

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use rustix::fs::{CWD, FileType, Mode, mknodat};
use secrecy::{ExposeSecret, SecretString};

use super::layout::{
    ADDRESS_KEY, ALICE, BOB, ENDPOINT, Layout, NEXT_ADDRESS_KEY, OTHER_WORKSPACE, T0, T1, T2,
    WORKSPACE, address, entry, fault, member, read, set_mode, workspace,
};
use crate::startup::{ADDRESS_KEY_CREDENTIAL, PREVIOUS_ADDRESS_KEY_CREDENTIAL};
use crate::{EntryFault, UnixNanos, Version, Written};

#[test]
#[ignore = "pending E8-14"]
fn a_cross_member_read_returns_none() {
    let layout = Layout::new();
    let writer = layout.writer();
    let reader = layout.reader();
    let ws = workspace(WORKSPACE);
    writer
        .put_if_absent(&ws, &address(1), &entry(ALICE, ENDPOINT, 1), T0)
        .unwrap();
    let got = reader
        .entry(&ws, &member(ALICE), &address(1))
        .unwrap()
        .expect("alice's own entry");
    assert_eq!(got.endpoint.expose_secret(), ENDPOINT);
    assert_eq!(
        got.keys.p256dh.expose_secret(),
        "p256dh-of-https://push.example/a"
    );
    assert_eq!(
        got.keys.auth.expose_secret(),
        "auth-of-https://push.example/a"
    );
    assert_eq!(got.key_id.expose_secret(), "vapid-key-1");
    assert_eq!(
        read(&reader, BOB, 1),
        Ok(false),
        "bob reads alice's entry as none"
    );
    assert_eq!(read(&reader, BOB, 2), Ok(false), "exactly as a missing one");
    assert!(
        matches!(
            reader.entry(&workspace(OTHER_WORKSPACE), &member(ALICE), &address(1)),
            Ok(None)
        ),
        "another workspace holds nothing under the same address_ref"
    );
}

#[test]
#[ignore = "pending E8-14"]
fn a_header_naming_another_member_is_none_but_a_tampered_one_is_an_error() {
    let layout = Layout::new();
    let writer = layout.writer();
    let reader = layout.reader();
    let ws = workspace(WORKSPACE);
    for n in [1, 2, 3] {
        writer
            .put_if_absent(&ws, &address(n), &entry(ALICE, ENDPOINT, n), T0)
            .unwrap();
    }
    let path = layout.entry_path(WORKSPACE, 1);
    let text = fs::read(&path).unwrap();
    let at = text
        .windows(ALICE.len())
        .position(|w| w == ALICE.as_bytes())
        .expect("cleartext member");
    let mut edited = text.clone();
    edited.splice(at..at + ALICE.len(), BOB.bytes());
    fs::write(&path, edited).unwrap();
    assert_eq!(
        read(&reader, ALICE, 1),
        Ok(false),
        "the header no longer names alice"
    );
    assert_eq!(
        read(&reader, BOB, 1),
        fault(1, EntryFault::Seal),
        "but bob's name was never sealed"
    );
    let path = layout.entry_path(WORKSPACE, 2);
    let mut flipped = fs::read(&path).unwrap();
    if let Some(last) = flipped.last_mut() {
        *last ^= 1;
    }
    fs::write(&path, flipped).unwrap();
    assert_eq!(read(&reader, ALICE, 2), fault(2, EntryFault::Seal));
    let path = layout.entry_path(WORKSPACE, 3);
    fs::write(&path, b"x").unwrap();
    assert_eq!(read(&reader, ALICE, 3), fault(3, EntryFault::Header));
}

#[test]
#[ignore = "pending E8-14"]
fn an_entry_moved_onto_another_name_fails_to_open() {
    let layout = Layout::new();
    let writer = layout.writer();
    let reader = layout.reader();
    let ws = workspace(WORKSPACE);
    writer
        .put_if_absent(&ws, &address(1), &entry(ALICE, ENDPOINT, 1), T0)
        .unwrap();
    writer
        .put_if_absent(
            &workspace(OTHER_WORKSPACE),
            &address(5),
            &entry(ALICE, ENDPOINT, 5),
            T0,
        )
        .unwrap();
    fs::copy(
        layout.entry_path(WORKSPACE, 1),
        layout.entry_path(WORKSPACE, 2),
    )
    .unwrap();
    set_mode(&layout.entry_path(WORKSPACE, 2), 0o640);
    assert_eq!(
        read(&reader, ALICE, 2),
        fault(2, EntryFault::Seal),
        "another address_ref"
    );
    fs::copy(
        layout.entry_path(OTHER_WORKSPACE, 5),
        layout.entry_path(WORKSPACE, 5),
    )
    .unwrap();
    set_mode(&layout.entry_path(WORKSPACE, 5), 0o640);
    assert_eq!(
        read(&reader, ALICE, 5),
        fault(5, EntryFault::Seal),
        "another workspace"
    );
}

#[test]
#[ignore = "pending E8-14"]
fn a_file_that_is_not_one_plain_entry_is_refused_without_blocking() {
    let layout = Layout::new();
    let writer = layout.writer();
    let reader = layout.reader();
    let ws = workspace(WORKSPACE);
    writer
        .put_if_absent(&ws, &address(1), &entry(ALICE, ENDPOINT, 1), T0)
        .unwrap();
    let path = |n: u8| layout.entry_path(WORKSPACE, n);
    fs::hard_link(path(1), layout.root.join("second-link")).unwrap();
    assert_eq!(
        read(&reader, ALICE, 1),
        fault(1, EntryFault::File),
        "two links"
    );
    fs::remove_file(layout.root.join("second-link")).unwrap();
    assert_eq!(read(&reader, ALICE, 1), Ok(true));
    set_mode(&path(1), 0o644);
    assert_eq!(
        read(&reader, ALICE, 1),
        fault(1, EntryFault::File),
        "readable by other"
    );
    set_mode(&path(1), 0o640);
    symlink(path(1), path(2)).unwrap();
    assert_eq!(
        read(&reader, ALICE, 2),
        fault(2, EntryFault::File),
        "a symlink"
    );
    mknodat(CWD, path(3), FileType::Fifo, Mode::from(0o640), 0).unwrap();
    assert_eq!(
        read(&reader, ALICE, 3),
        fault(3, EntryFault::File),
        "a FIFO, without blocking"
    );
    fs::create_dir(path(4)).unwrap();
    assert_eq!(
        read(&reader, ALICE, 4),
        fault(4, EntryFault::File),
        "a directory"
    );
}

/// Sets a file's modification time, the sweep's second clock (DEC-2120 item 7).
fn touch(path: &Path, at: UnixNanos) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_nanos(at.0))
        .unwrap();
}

#[test]
#[ignore = "pending E8-14"]
fn the_sweep_lists_old_entries_by_header_and_file_and_removes_stale_temp_files() {
    let layout = Layout::new();
    let writer = layout.writer();
    let ws = workspace(WORKSPACE);
    for (n, at) in [(1, T0), (2, T2), (3, T1)] {
        writer
            .put_if_absent(&ws, &address(n), &entry(ALICE, ENDPOINT, n), at)
            .unwrap();
    }
    let dir = layout.workspace_dir(WORKSPACE);
    let temp = dir.join(".tmp-0123456789abcdef");
    for name in [
        ".tmp-0123456789abcdef",
        "NOT-A-ULID",
        "01j8z2r0000000000000000009",
    ] {
        fs::copy(layout.entry_path(WORKSPACE, 1), dir.join(name)).unwrap();
        set_mode(&dir.join(name), 0o640);
        touch(&dir.join(name), T0);
    }
    for (n, at) in [(1, T0), (2, T2), (3, T0)] {
        touch(&layout.entry_path(WORKSPACE, n), at);
    }
    touch(&temp, T2);
    let written = |n: u8, at: UnixNanos| Written {
        address_ref: address(n),
        version: Version(1),
        written_at: at,
    };
    assert_eq!(
        writer.inert_older_than(&ws, T1, T1),
        Ok(vec![written(1, T0)]),
        "entry 2's header is ahead of now but its file is fresh, so it may be a write in flight; \
         entry 3 was written at the cutoff"
    );
    assert!(
        temp.exists(),
        "a temp file newer than the cutoff may be a write in progress"
    );
    touch(&layout.entry_path(WORKSPACE, 2), T0);
    touch(&temp, T0);
    assert_eq!(
        writer.inert_older_than(&ws, T1, T1),
        Ok(vec![written(1, T0), written(2, T2)]),
        "a header ahead of now on an old file is a candidate, never a reason to keep the entry"
    );
    assert!(!temp.exists(), "a stale temp file is unlinked");
    assert!(
        dir.join("NOT-A-ULID").exists(),
        "a stray name is neither an entry nor deleted"
    );
    assert!(dir.join("01j8z2r0000000000000000009").exists());
}

#[test]
#[ignore = "pending E8-14"]
fn a_rotation_reads_both_keys_reseals_on_write_and_retires_the_old_one() {
    let layout = Layout::new();
    let ws = workspace(WORKSPACE);
    let writer = layout.writer();
    for n in [1, 3] {
        writer
            .put_if_absent(&ws, &address(n), &entry(ALICE, ENDPOINT, n), T0)
            .unwrap();
    }
    layout.install_key(ADDRESS_KEY_CREDENTIAL, NEXT_ADDRESS_KEY);
    layout.install_key(PREVIOUS_ADDRESS_KEY_CREDENTIAL, ADDRESS_KEY);
    let (writer, reader) = (layout.writer(), layout.reader());
    assert_eq!(
        read(&reader, ALICE, 1),
        Ok(true),
        "the previous key still opens it"
    );
    let endpoint = SecretString::from(ENDPOINT.to_owned());
    assert_eq!(
        writer
            .entries_holding(&ws, &member(ALICE), &endpoint)
            .map(|h| h.len()),
        Ok(2)
    );
    writer
        .put_again(&ws, &address(1), &entry(ALICE, ENDPOINT, 1), T1)
        .unwrap();
    writer
        .put_if_absent(&ws, &address(2), &entry(ALICE, ENDPOINT, 2), T1)
        .unwrap();
    fs::remove_file(layout.credentials.join(PREVIOUS_ADDRESS_KEY_CREDENTIAL)).unwrap();
    let reader = layout.reader();
    assert_eq!(
        read(&reader, ALICE, 1),
        Ok(true),
        "re-sealed under the current key"
    );
    assert_eq!(read(&reader, ALICE, 2), Ok(true));
    assert_eq!(
        read(&reader, ALICE, 3),
        fault(3, EntryFault::RetiredKey),
        "address_missing"
    );
}
