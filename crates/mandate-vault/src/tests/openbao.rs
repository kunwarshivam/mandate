//! A sealed vault or a dev server is refused before any login (DEC-977 items 5 and 6).

use crate::SourceError;
use crate::openbao::seal_verdict;

const UNSEALED_RAFT: &[u8] =
    br#"{"type":"shamir","initialized":true,"sealed":false,"t":3,"n":5,"storage_type":"raft"}"#;
const UNSEALED_DEV: &[u8] =
    br#"{"type":"shamir","initialized":true,"sealed":false,"t":1,"n":1,"storage_type":"inmem"}"#;
const SEALED: &[u8] =
    br#"{"type":"shamir","initialized":true,"sealed":true,"t":3,"n":5,"storage_type":"raft"}"#;

#[test]
fn a_production_server_passes_and_a_dev_server_needs_the_test_allowance() {
    assert_eq!(seal_verdict(UNSEALED_RAFT, false), Ok(()));
    assert_eq!(seal_verdict(UNSEALED_RAFT, true), Ok(()));
    assert_eq!(
        seal_verdict(UNSEALED_DEV, false),
        Err(SourceError::DevServerRefused)
    );
    assert_eq!(seal_verdict(UNSEALED_DEV, true), Ok(()));
}

#[test]
fn a_sealed_or_unreadable_status_is_refused() {
    for allow_dev in [false, true] {
        assert_eq!(seal_verdict(SEALED, allow_dev), Err(SourceError::Sealed));
        assert_eq!(
            seal_verdict(
                br#"{"initialized":false,"sealed":false,"storage_type":"raft"}"#,
                allow_dev
            ),
            Err(SourceError::Sealed),
            "an uninitialized vault waits for its operator as a sealed one does"
        );
        for bad in [
            &b""[..],
            b"[]",
            br#"{"sealed":"no","storage_type":"raft"}"#,
            br#"{"storage_type":"raft"}"#,
            br#"{"sealed":false}"#,
            br#"{"sealed":false,"storage_type":7}"#,
        ] {
            assert_eq!(
                seal_verdict(bad, allow_dev),
                Err(SourceError::Malformed),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }
}
