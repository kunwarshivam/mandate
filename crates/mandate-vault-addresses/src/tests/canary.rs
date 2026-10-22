//! No endpoint, key, `key_id`, digest, or key material reaches an error, a `Display`, or a `Debug`
//! output (DEC-2120 item 5, NT-2). Every value planted carries `CANARY`; the digests and the
//! address key are runs of one byte, which a printed array would show as `n, n, n`.

use std::fs;

use secrecy::{SecretBox, SecretString};

use super::layout::{
    ALICE, BOB, Layout, T0, T1, T2, WORKSPACE, address, member, set_mode, workspace,
};
use crate::startup::open_reader;
use crate::{AddressRef, MemberId, NewEntry, PushKeys, UnixNanos, Version, WorkspaceId};

const CANARY_ENDPOINT: &str = "https://push.example/CANARY-endpoint-7f3a";

fn canary_entry(who: &str, digest: u8) -> NewEntry {
    NewEntry {
        member: member(who),
        endpoint: SecretString::from(CANARY_ENDPOINT.to_owned()),
        keys: PushKeys {
            p256dh: SecretString::from("CANARY-p256dh".to_owned()),
            auth: SecretString::from("CANARY-auth".to_owned()),
        },
        key_id: SecretString::from("CANARY-key-id".to_owned()),
        request_digest: SecretBox::new(Box::new([digest; 32])),
    }
}

#[test]
#[ignore = "pending E8-14"]
fn no_endpoint_or_key_appears_in_any_error_or_debug_output() {
    let layout = Layout::new();
    let writer = layout.writer();
    let reader = layout.reader();
    let ws = workspace(WORKSPACE);
    let one = address(1);
    let mut seen = vec![format!("{writer:?}"), format!("{reader:?}")];
    let written = canary_entry(ALICE, 7);
    seen.push(format!("{written:?}"));
    seen.push(format!(
        "{:?}",
        writer.put_if_absent(&ws, &one, &written, T0).unwrap()
    ));
    seen.push(format!(
        "{:?}",
        writer.put_again(&ws, &one, &written, T1).unwrap()
    ));
    let conflict = writer
        .put_if_absent(&ws, &one, &canary_entry(BOB, 8), T2)
        .unwrap_err();
    seen.extend([format!("{conflict}"), format!("{conflict:?}")]);
    let held = writer
        .entries_holding(
            &ws,
            &member(ALICE),
            &SecretString::from(CANARY_ENDPOINT.to_owned()),
        )
        .unwrap();
    assert_eq!(
        held.len(),
        1,
        "the canary entry is found, so its output is what is scanned"
    );
    seen.push(format!("{held:?}"));
    seen.push(format!(
        "{:?}",
        writer
            .inert_older_than(&ws, UnixNanos(u64::MAX), T2)
            .unwrap()
    ));
    let read = reader
        .entry(&ws, &member(ALICE), &one)
        .unwrap()
        .expect("the canary entry");
    seen.push(format!("{read:?}"));
    let entry_path = layout.entry_path(WORKSPACE, 1);
    set_mode(&entry_path, 0o644);
    let file_fault = reader.entry(&ws, &member(ALICE), &one).unwrap_err();
    set_mode(&entry_path, 0o640);
    let mut sealed = fs::read(&entry_path).unwrap();
    if let Some(last) = sealed.last_mut() {
        *last ^= 1;
    }
    fs::write(&entry_path, sealed).unwrap();
    let seal_fault = reader.entry(&ws, &member(ALICE), &one).unwrap_err();
    for error in [file_fault, seal_fault] {
        seen.extend([format!("{error}"), format!("{error:?}")]);
    }
    for text in [CANARY_ENDPOINT, "CANARY-ws"] {
        for refused in [
            WorkspaceId::parse(text).map(drop),
            AddressRef::parse(text).map(drop),
            MemberId::parse(text).map(drop),
        ] {
            let error = refused.unwrap_err();
            seen.extend([format!("{error}"), format!("{error:?}")]);
        }
    }
    let mut leaky = Layout::new();
    leaky.environment.push((
        "MANDATE_VAULT_ADDRESS_KEY".to_owned(),
        "CANARY-env".to_owned(),
    ));
    let error = open_reader(&leaky.inputs()).unwrap_err();
    seen.extend([format!("{error}"), format!("{error:?}")]);
    seen.push(format!("{:?}", Version(1)));
    for output in &seen {
        for needle in ["CANARY", "7f3a", "7, 7, 7", "8, 8, 8", "5, 5, 5"] {
            assert!(!output.contains(needle), "`{needle}` leaked into: {output}");
        }
    }
}
