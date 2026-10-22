//! The error vocabulary: every message is fixed text, so none can carry an input (DEC-2120 item 5).

use std::io::ErrorKind;

use crate::{AddressError, EntryFault};

#[test]
fn every_error_message_is_fixed_text() {
    let messages = [
        (
            AddressError::Unimplemented { story: "E8-14" },
            "E8-14 has not been implemented yet",
        ),
        (
            AddressError::KeyInEnvironment,
            "a vault key is set in the environment; keys arrive only as systemd credentials",
        ),
        (
            AddressError::KeyCredential,
            "the address key credential is missing, not a regular file, not 32 bytes, or readable by group or other",
        ),
        (
            AddressError::PendingKeyPresent,
            "the pending key is among the dispatcher's credentials",
        ),
        (
            AddressError::TokenKeyPresent,
            "the token key is among this process's credentials",
        ),
        (
            AddressError::Layout { dir: "addresses" },
            "the addresses directory has the wrong owner, group, or mode, or is a symlink",
        ),
        (
            AddressError::InvalidId {
                kind: "address_ref",
            },
            "the id is not a valid address_ref",
        ),
        (AddressError::Random, "the secure random source failed"),
        (
            AddressError::Io {
                kind: ErrorKind::NotFound,
            },
            "the address store could not be read or written",
        ),
    ];
    for (error, text) in messages {
        assert_eq!(error.to_string(), text);
    }
    assert_ne!(EntryFault::File, EntryFault::Seal);
}
