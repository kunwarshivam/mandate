//! The connect check of an exchanged OAuth connection (DEC-2339; connections spec §5.2 step 4,
//! §8.1 checks 1, 2 and 3; journal spec §9.8): run by the connection's own executor once the token
//! exchange has stored its token. The account id is read from the paper host, written once to the
//! connection's personal-data entry under a fresh `pii_` reference, and never journaled; the
//! `ConnectionChecked` record holds the reference alone.
//!
//! Creating the entry is the one step that cannot be repeated: the executor may create its entry
//! but never read it. So any refusal of the create, a second create after a crash before the record
//! included, journals the account as unreadable, a null `account_pii_ref` (rule 62), and the API
//! refuses the connect; the owner reconnects under a new connection id (DEC-2339 item 6).

use mandate_connections::account_entry::BrokerAccountId;
use mandate_connections::grant::GrantedScopes;
use mandate_connections::record::{AccountPiiRef, ConnectionId};
use mandate_executor::{AccountScope, IdGen};
use mandate_store::Store;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::policy::SecretPath;
use secrecy::SecretSlice;

use crate::connect::Checked;

/// The report of a function E21-32 has not implemented yet, which its pending tests fail on
/// (DEC-77); the implementation PR replaces the stubs and removes it.
#[allow(
    non_upper_case_globals,
    reason = "the pending gate recognises a stub by the name `Unimplemented` (DEC-137)"
)]
pub const Unimplemented: &str = "unimplemented";

/// What the OAuth connect check is given: the connection, the scopes its token entry was stored
/// with, the account id the paper host answered (`None` when it could not be read), and the
/// reference to keep it under.
#[derive(Debug)]
pub struct OAuthCheck {
    pub connection: ConnectionId,
    pub granted: GrantedScopes,
    pub account: Option<BrokerAccountId>,
    pub pii_ref: AccountPiiRef,
}

/// Runs `check` for the account in `scope` at `now`: once the account stream is this writer's, an
/// account read is written by `create` to the connection's [`SecretPath::AccountId`] entry, in
/// [`mandate_connections::account_entry::encode`]'s form, and the checks run as an Alpaca paper
/// OAuth connection. The account is read only when `create` stored the entry; an unread account
/// never calls it, and any refusal of it is an unreadable account.
///
/// # Errors
/// [`crate::connect`]'s journaling codes: `stream_unavailable`, `check_refused` and
/// `append_refused`.
pub fn oauth_connect_check<S: Store>(
    store: &mut S,
    ids: &dyn IdGen,
    scope: &AccountScope,
    check: OAuthCheck,
    create: impl FnOnce(&SecretPath, &SecretSlice<u8>) -> Result<(), SourceError>,
    now: UtcNanos,
) -> Result<Checked, &'static str> {
    let _ = (store, ids, scope, check, create, now);
    Err(Unimplemented)
}

/// The scopes a leased secret's token entry was stored with (DEC-2624), by its one reader.
///
/// # Errors
/// `credential_malformed` for bytes that are not a token entry; it names nothing of them.
pub fn leased_scopes(secret: &[u8]) -> Result<GrantedScopes, &'static str> {
    let _ = secret;
    Err(Unimplemented)
}
