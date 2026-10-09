//! The granted scopes must equal the requested ones (connections spec §5.3, §8.1 check 1;
//! DEC-821 item 4). The token-exchange process checks them on the token response, before the
//! vault write, so an over-scoped token is never stored (CN-2).

use std::collections::BTreeSet;

use crate::ConnectError;

/// The requested scopes, sorted, each exactly once. The grant is split on single spaces only, so
/// any other separator, a doubled space, or an edge space leaves a part that matches nothing here.
const REQUESTED_SCOPES_SORTED: [&str; 2] = ["data", "trading"];

/// The scopes Alpaca granted, once they equal the request (`trading` and `data`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantedScopes(pub(crate) BTreeSet<String>);

/// Reads the token response's space-separated `scope` and accepts it only if it is exactly
/// `trading` and `data`, in any order: a missing or an added scope is refused (CN-2).
pub fn check_scope(granted: &str) -> Result<GrantedScopes, ConnectError> {
    let mut scopes: Vec<&str> = granted.split(' ').collect();
    scopes.sort_unstable();
    if scopes != REQUESTED_SCOPES_SORTED {
        return Err(ConnectError::ScopeMismatch);
    }
    Ok(GrantedScopes(
        scopes.into_iter().map(str::to_owned).collect(),
    ))
}
