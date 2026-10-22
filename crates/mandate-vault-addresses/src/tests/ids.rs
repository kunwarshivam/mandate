//! Ids name exactly one directory or file, so no input reaches outside its workspace
//! (DEC-2120 item 3).

use crate::{AddressError, AddressRef, MemberId, WorkspaceId};

#[test]
#[ignore = "pending E8-14"]
fn ids_that_are_not_one_path_component_or_not_canonical_are_refused() {
    let long = "w".repeat(65);
    for text in [
        "",
        ".",
        "..",
        "../ws",
        "ws/1",
        "ws\\1",
        "ws 1",
        "ws.1",
        ".lock",
        long.as_str(),
    ] {
        assert_eq!(
            WorkspaceId::parse(text),
            Err(AddressError::InvalidId {
                kind: "workspace id"
            })
        );
    }
    assert!(WorkspaceId::parse(&"w".repeat(64)).is_ok());
    let canonical = "01J8Z2A0000000000000000001";
    let refused = [
        "",
        ".tmp-0123456789abcdef",
        "01j8z2a0000000000000000001",
        "81J8Z2A0000000000000000001",
        "01J8Z2A000000000000000000I",
        "01J8Z2A00000000000000000001",
        "01J8Z2A000000000000000000",
    ];
    for text in refused {
        assert_eq!(
            AddressRef::parse(text),
            Err(AddressError::InvalidId {
                kind: "address_ref"
            })
        );
        assert_eq!(
            MemberId::parse(text),
            Err(AddressError::InvalidId { kind: "member id" })
        );
    }
    assert!(AddressRef::parse(canonical).is_ok() && MemberId::parse(canonical).is_ok());
}
