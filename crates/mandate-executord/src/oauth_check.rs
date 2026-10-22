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

use mandate_connections::account_entry::{BrokerAccountId, encode};
use mandate_connections::checks::{AccountRead, CheckInput, Granted, Occasion, run};
use mandate_connections::grant::GrantedScopes;
use mandate_connections::record::{AccountPiiRef, AuthKind, Broker, ConnectionId, Environment};
use mandate_connections::token_entry;
use mandate_connections::vault::AccessToken;
use mandate_executor::{AccountScope, IdGen};
use mandate_store::Store;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::personal::{PiiRandom, mint_pii_ref};
use mandate_vault::policy::SecretPath;
use mandate_vault::source::SecretSource;
use secrecy::{ExposeSecret, SecretSlice};

use crate::connect::{Checked, journal_check};
use crate::credential::Unimplemented;

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
    let OAuthCheck {
        connection,
        granted,
        account,
        pii_ref,
    } = check;
    journal_check(store, ids, scope, now, || {
        let stored = account.is_some_and(|id| {
            encode(&pii_ref, &id).is_ok_and(|entry| {
                let bytes = SecretSlice::from(entry.expose_secret().as_bytes().to_vec());
                create(&SecretPath::AccountId(connection.clone()), &bytes).is_ok()
            })
        });
        let account = if stored {
            AccountRead::Read {
                pii_ref,
                dedicated: None,
            }
        } else {
            AccountRead::Unreadable
        };
        run(&CheckInput {
            connection_id: connection,
            broker: Broker::Alpaca,
            environment: Environment::Paper,
            auth_kind: AuthKind::Oauth,
            occasion: Occasion::Connect,
            granted: Granted::OAuth(granted),
            account,
            contract: None,
            pinned_contract: None,
        })
        .map_err(|_| "check_refused")
    })
}

/// The scopes a leased secret's token entry was stored with (DEC-2624), by its one reader.
///
/// # Errors
/// `credential_malformed` for bytes that are not a token entry; it names nothing of them.
pub fn leased_scopes(secret: &[u8]) -> Result<GrantedScopes, &'static str> {
    token_entry::decode(secret)
        .map(|entry| entry.scopes)
        .map_err(|_| "credential_malformed")
}

/// What the exchanged executor holds for its connect check: its connection, the secret its lease
/// read, and the account id the paper host answered (`None` when it could not be read).
#[derive(Debug)]
pub struct Exchanged<'a> {
    pub connection: ConnectionId,
    pub leased: &'a [u8],
    pub account: Option<BrokerAccountId>,
}

/// The exchanged executor's connect check from what it holds: the scopes its lease's token entry
/// was stored with, and a reference minted from `random` at `now`, then [`oauth_connect_check`]
/// with `create`.
///
/// # Errors
/// `credential_malformed` for a secret that is not a token entry, `entropy_unavailable` when
/// `random` cannot fill, and [`oauth_connect_check`]'s codes. Neither the secret nor the id is
/// named.
pub fn exchanged_check<S: Store>(
    store: &mut S,
    ids: &dyn IdGen,
    scope: &AccountScope,
    held: Exchanged<'_>,
    random: &mut dyn PiiRandom,
    create: impl FnOnce(&SecretPath, &SecretSlice<u8>) -> Result<(), SourceError>,
    now: UtcNanos,
) -> Result<Checked, &'static str> {
    let Exchanged {
        connection,
        leased,
        account,
    } = held;
    let granted = leased_scopes(leased)?;
    let pii_ref = mint_pii_ref(now, random).map_err(|_| "entropy_unavailable")?;
    oauth_connect_check(
        store,
        ids,
        scope,
        OAuthCheck {
            connection,
            granted,
            account,
            pii_ref,
        },
        create,
        now,
    )
}

/// What the exchanged executor starts with: its connection, the vault session it logged in as,
/// the paper host's account read, which is handed the leased access token, and its random source.
pub struct Start<'a, V, A> {
    pub connection: &'a ConnectionId,
    pub vault: &'a V,
    pub account: A,
    pub random: &'a mut dyn PiiRandom,
}

/// The exchanged executor's start (DEC-2339; connections spec §5.2 step 4). While the account
/// stream in `scope` holds no `connect` check, it leases the connection's secret from the vault,
/// hands the account read the access token that lease holds, runs [`exchanged_check`] over the
/// lease's own bytes, creating the entry through the vault, and gives the lease back; it answers
/// the check. A stream that already holds a `connect` check, passed or failed, runs none and
/// answers `None`, so a refused connect is never run again into a second create.
///
/// # Errors
/// `connection_record_unreadable` for a check record that does not read,
/// `credential_unavailable` when the vault refuses the lease, and [`exchanged_check`]'s codes,
/// `entropy_unavailable` among them: each refuses the start before any create, and none names
/// the secret or the id.
pub fn exchanged_start<S, V, A>(
    store: &mut S,
    ids: &dyn IdGen,
    scope: &AccountScope,
    start: Start<'_, V, A>,
    now: UtcNanos,
) -> Result<Option<Checked>, &'static str>
where
    S: Store,
    V: SecretSource,
    A: FnOnce(AccessToken) -> Option<BrokerAccountId>,
{
    let _ = (store, ids, scope, start, now);
    Err(Unimplemented)
}
