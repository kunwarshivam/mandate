//! Whether the process binds a connect, from its settings
//! ([DEC-2338](../../../docs/project/decisions/DEC-2338.md) item 5): only when the four connect
//! settings are all set and valid. None of them set is no connect, as before; some set is no
//! connect too, named at start by setting, never by value. The authorization page's own gate
//! ([`crate::config::authorize_page`], on [`crate::config::AUTHORIZE_URL`]) is judged first and still refuses the start.

use std::ffi::OsString;
use std::sync::Arc;

use mandate_connections::start::Authorizer;
use mandate_release::startup::Tier;

use crate::clock::Clock;
use crate::config::StartRefusal;
use crate::connect::Connect;
use crate::session::Random;
use crate::vault_codes::OpenBaoLogin;

/// The platform's registered OAuth client id.
pub const CLIENT_ID: &str = "MANDATE_ALPACA_CLIENT_ID";

/// The deployment's registered redirect URI.
pub const REDIRECT_URI: &str = "MANDATE_ALPACA_REDIRECT_URI";

/// The vault's `https://host:port`.
pub const VAULT_ADDRESS: &str = "MANDATE_API_VAULT_ADDRESS";

/// The directory holding `ca.crt`, `api.role-id` and `api.secret-id`, each readable by the process
/// alone.
pub const VAULT_DIR: &str = "MANDATE_API_VAULT_DIR";

/// The four settings a connect needs, in the order they are judged and named.
pub const CONNECT_SETTINGS: [&str; 4] = [CLIENT_ID, REDIRECT_URI, VAULT_ADDRESS, VAULT_DIR];

/// The vault's CA certificate, in [`VAULT_DIR`].
pub const VAULT_CA: &str = "ca.crt";

/// The longest client id accepted, in bytes.
pub const MAX_CLIENT_ID_LEN: usize = 256;

/// What the settings bind.
#[derive(Debug)]
pub enum ConnectBinding {
    /// None of [`CONNECT_SETTINGS`] is set: no connect, and nothing to say.
    Off,
    /// Some are set but not all valid: no connect, and these, by name, are why.
    Incomplete(Vec<&'static str>),
    /// Every one is set and valid.
    On(Box<BoundConnect>),
}

/// What a bound connect starts with: its authorization page and client, and its vault login.
#[derive(Debug)]
pub struct BoundConnect {
    pub authorizer: Authorizer,
    pub login: OpenBaoLogin,
}

impl ConnectBinding {
    /// The one line the start writes for [`ConnectBinding::Incomplete`], naming settings only.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn off_line(&self) -> Option<String> {
        todo!()
    }
}

/// The binding `var` gives in `tier`.
///
/// # Errors
/// [`authorize_page`]'s refusal, judged before the connect settings.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn connect_binding(
    _tier: Tier,
    _var: impl Fn(&str) -> Option<OsString>,
) -> Result<ConnectBinding, StartRefusal> {
    todo!()
}

/// The connect `binding` binds: for [`ConnectBinding::On`], its page and client, states drawn from
/// `random`, and codes written through the vault as the API's identity at `clock`'s time; `None`
/// otherwise.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn bound(
    _binding: ConnectBinding,
    _random: Box<dyn Random>,
    _clock: Arc<dyn Clock>,
) -> Option<Arc<Connect>> {
    todo!()
}
