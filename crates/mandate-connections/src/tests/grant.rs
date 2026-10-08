//! The granted scopes equal the request exactly (connections spec §5.3; DEC-821 item 4).

use std::collections::BTreeSet;

use crate::ConnectError;
use crate::grant::{GrantedScopes, check_scope};

#[test]
#[ignore = "pending E10-13"]
fn granted_scopes_must_equal_the_request() {
    let both = GrantedScopes(BTreeSet::from(["data".to_owned(), "trading".to_owned()]));
    for granted in ["trading data", "data trading"] {
        assert_eq!(check_scope(granted), Ok(both.clone()), "{granted:?}");
    }
    for granted in [
        "",
        " ",
        "trading",
        "data",
        "trading data foo",
        "trading data account:read",
        "trading data account:write",
        "data trading account:write",
        "account:write trading",
        "trading data data",
        "trading trading",
        " trading data",
        "trading data ",
        "trading  data",
        "trading\tdata",
        "trading\ndata",
        "Trading data",
        "trading DATA",
        "trading,data",
        "trading\rdata",
        "trading\u{000B}data",
        "trading\u{000C}data",
        "trading\u{0085}data",
        "trading\u{00A0}data",
        "trading\u{2003}data",
        "trading\u{2028}data",
        "trading\u{3000}data",
        "trading\u{200B}data",
        "\u{FEFF}trading data",
        "trading data\u{00A0}",
        "trading\0data",
        "trading data\0",
        "\0trading data",
        "trading\0 data",
        "trad\u{0456}ng data",
    ] {
        assert_eq!(
            check_scope(granted),
            Err(ConnectError::ScopeMismatch),
            "{granted:?}"
        );
    }
}
