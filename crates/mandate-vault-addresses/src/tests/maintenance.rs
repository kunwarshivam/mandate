//! The re-seal tool (DEC-2120 item 6).

use std::fs;

use secrecy::SecretString;

use super::layout::{
    ADDRESS_KEY, ALICE, BOB, ENDPOINT, Layout, NEXT_ADDRESS_KEY, OTHER_WORKSPACE, T0, T1, T2,
    WORKSPACE, address, entry, member, read, workspace,
};
use crate::maintenance::{KeyCounts, Resealed, Unopenable, open_maintenance};
use crate::startup::{
    ADDRESS_KEY_CREDENTIAL, PREVIOUS_ADDRESS_KEY_CREDENTIAL, TOKEN_KEY_CREDENTIAL,
};
use crate::{AddressError, Erasure, UnixNanos, Version, Written};

/// Writes `entries` under the first key, then rotates: the next key current, the first previous.
fn rotated(layout: &Layout, entries: &[(&str, u8)]) {
    let writer = layout.writer();
    for (ws, n) in entries {
        let who = if *n % 2 == 0 { BOB } else { ALICE };
        writer
            .put_if_absent(&workspace(ws), &address(*n), &entry(who, ENDPOINT, *n), T0)
            .unwrap();
    }
    layout.install_key(ADDRESS_KEY_CREDENTIAL, NEXT_ADDRESS_KEY);
    layout.install_key(PREVIOUS_ADDRESS_KEY_CREDENTIAL, ADDRESS_KEY);
}

#[test]
#[ignore = "pending E8-14"]
fn reseal_moves_every_entry_to_the_current_key() {
    let layout = Layout::new();
    rotated(
        &layout,
        &[(WORKSPACE, 1), (WORKSPACE, 2), (OTHER_WORKSPACE, 3)],
    );
    let tool = layout.maintenance();
    let counts = |under_current, under_previous| {
        Ok(KeyCounts {
            under_current,
            under_previous,
            unopenable: vec![],
        })
    };
    assert_eq!(tool.count_by_key_version(), counts(0, 3));
    assert_eq!(
        tool.reseal(T1),
        Ok(Resealed {
            resealed: 3,
            unopenable: vec![]
        })
    );
    assert_eq!(
        tool.count_by_key_version(),
        counts(3, 0),
        "the previous key may now retire"
    );
    assert_eq!(
        tool.reseal(T2),
        Ok(Resealed {
            resealed: 0,
            unopenable: vec![]
        })
    );
    fs::remove_file(layout.credentials.join(PREVIOUS_ADDRESS_KEY_CREDENTIAL)).unwrap();
    let reader = layout.reader();
    assert_eq!(read(&reader, ALICE, 1), Ok(true));
    assert_eq!(read(&reader, BOB, 2), Ok(true));
    let writer = layout.writer();
    assert_eq!(
        writer.inert_older_than(&workspace(WORKSPACE), UnixNanos(T1.0 + 1), T1),
        Ok(vec![
            Written {
                address_ref: address(1),
                version: Version(2),
                written_at: T1
            },
            Written {
                address_ref: address(2),
                version: Version(2),
                written_at: T1
            },
        ]),
        "a re-seal is a write: it bumps the version and the time"
    );
    let endpoint = SecretString::from(ENDPOINT.to_owned());
    assert_eq!(
        writer
            .entries_holding(&workspace(WORKSPACE), &member(ALICE), &endpoint)
            .map(|h| h.len()),
        Ok(1),
        "the endpoint hash was recomputed under the current key"
    );
}

#[test]
#[ignore = "pending E8-14"]
fn an_entry_the_tool_cannot_open_is_reported_kept_and_blocks_retirement() {
    let layout = Layout::new();
    rotated(&layout, &[(WORKSPACE, 1), (WORKSPACE, 2)]);
    let path = layout.entry_path(WORKSPACE, 2);
    let mut sealed = fs::read(&path).unwrap();
    if let Some(last) = sealed.last_mut() {
        *last ^= 1;
    }
    fs::write(&path, sealed).unwrap();
    let tool = layout.maintenance();
    let unopenable = vec![Unopenable {
        workspace: workspace(WORKSPACE),
        address_ref: address(2),
    }];
    assert_eq!(
        tool.reseal(T1),
        Ok(Resealed {
            resealed: 1,
            unopenable
        })
    );
    assert!(path.exists(), "the tool never erases");
    assert_eq!(
        tool.count_by_key_version()
            .map(|c| (c.under_current, c.under_previous)),
        Ok((1, 1)),
        "the corrupt entry still counts under the previous key, so it cannot retire"
    );
    assert_eq!(
        layout.writer().erase(&workspace(WORKSPACE), &address(2)),
        Ok(Erasure::Erased)
    );
    assert_eq!(
        tool.count_by_key_version()
            .map(|c| (c.under_current, c.under_previous)),
        Ok((1, 0))
    );
}

#[test]
#[ignore = "pending E8-14"]
fn the_tool_opens_under_the_writers_refusals() {
    let mut layout = Layout::new();
    layout
        .environment
        .push(("MANDATE_VAULT_KEY".to_owned(), String::new()));
    assert_eq!(
        open_maintenance(&layout.inputs()).map(drop),
        Err(AddressError::KeyInEnvironment)
    );
    let layout = Layout::new();
    fs::write(layout.credentials.join(TOKEN_KEY_CREDENTIAL), [9; 32]).unwrap();
    assert_eq!(
        open_maintenance(&layout.inputs()).map(drop),
        Err(AddressError::TokenKeyPresent)
    );
    let layout = Layout::new();
    assert_eq!(open_maintenance(&layout.inputs()).map(drop), Ok(()));
}
