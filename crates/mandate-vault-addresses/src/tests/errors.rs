//! The error vocabulary: every message is fixed text, so none can carry an input, not even the
//! opaque id a variant holds (DEC-2120 item 8).

use std::io::ErrorKind;

use crate::{AddressError, AddressRef, EntryFault};

/// Each variant's message. The match has no wildcard, so a new variant fails to compile here until
/// its message is pinned.
fn message_of(error: &AddressError) -> &'static str {
    match error {
        AddressError::Unimplemented { .. } => "E8-14 has not been implemented yet",
        AddressError::KeyInEnvironment => {
            "a vault key is set in the environment; keys arrive only as systemd credentials"
        }
        AddressError::KeyCredential => {
            "the address key credential is missing, not a regular file, not 32 bytes, or readable by group or other"
        }
        AddressError::TokenKeyPresent => "the token key is among this process's credentials",
        AddressError::PendingKeyPresent => "the pending key is among the dispatcher's credentials",
        AddressError::Layout { .. } => {
            "the addresses directory has the wrong owner, group, or mode, or is a symlink"
        }
        AddressError::InvalidId { .. } => "the id is not a valid address_ref",
        AddressError::Entry { .. } => "an address entry could not be opened",
        AddressError::Conflict { .. } => "the address entry is held for another request or member",
        AddressError::Random => "the secure random source failed",
        AddressError::Io { .. } => "the address store could not be read or written",
    }
}

#[test]
fn every_error_message_is_fixed_text() {
    let address_ref = AddressRef::unchecked("01J8Z2R0000000000000000001");
    let errors = [
        AddressError::Unimplemented { story: "E8-14" },
        AddressError::KeyInEnvironment,
        AddressError::KeyCredential,
        AddressError::TokenKeyPresent,
        AddressError::PendingKeyPresent,
        AddressError::Layout { dir: "addresses" },
        AddressError::InvalidId {
            kind: "address_ref",
        },
        AddressError::Entry {
            address_ref: address_ref.clone(),
            fault: EntryFault::Seal,
        },
        AddressError::Conflict {
            address_ref: address_ref.clone(),
        },
        AddressError::Random,
        AddressError::Io {
            kind: ErrorKind::NotFound,
        },
    ];
    for error in &errors {
        let text = error.to_string();
        assert_eq!(text, message_of(error));
        assert!(!text.contains("01J8Z2R"), "an id leaked into: {text}");
    }
    assert_ne!(EntryFault::File, EntryFault::Seal);
}
