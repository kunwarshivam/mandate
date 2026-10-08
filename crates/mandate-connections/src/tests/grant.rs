//! The granted scopes equal the request exactly (connections spec §5.3; DEC-821 item 3).

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
    ] {
        assert_eq!(
            check_scope(granted),
            Err(ConnectError::ScopeMismatch),
            "{granted:?}"
        );
    }
}
