//! Whether the process binds a connect, from its settings
//! ([DEC-2338](../../../docs/project/decisions/DEC-2338.md) item 5): only when the four connect
//! settings are all set and valid. None of them set is no connect, as before; some set is no
//! connect too, named at start by setting, never by value. The authorization page's own gate
//! ([`crate::config::authorize_page`]) is judged first and still refuses the start.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mandate_connections::start::{Authorizer, ClientId, OAuthClient, RedirectUri};
use mandate_release::startup::Tier;
use mandate_vault::mounted::read_mounted;
use mandate_vault::openbao::OpenBaoConfig;
use mandate_vault::policy::Mounts;
use secrecy::ExposeSecret;

use crate::clock::Clock;
use crate::config::{AUTHORIZE_URL, StartRefusal, authorize_page};
use crate::connect::Connect;
use crate::session::Random;
use crate::vault_codes::{OpenBaoLogin, VaultCodes};

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
    pub fn off_line(&self) -> Option<String> {
        match self {
            Self::Incomplete(names) => Some(format!(
                "connect off: {} missing or invalid",
                names.join(" ")
            )),
            Self::Off | Self::On(_) => None,
        }
    }
}

/// The binding `var` gives in `tier`.
///
/// # Errors
/// [`authorize_page`]'s refusal, judged before the connect settings.
pub fn connect_binding(
    tier: Tier,
    var: impl Fn(&str) -> Option<OsString>,
) -> Result<ConnectBinding, StartRefusal> {
    let page = authorize_page(tier, var(AUTHORIZE_URL).as_deref())?;
    let text = |name: &str| var(name).map(|value| value.into_string().ok());
    let set = CONNECT_SETTINGS.map(|name| var(name).is_some());
    if !set.contains(&true) {
        return Ok(ConnectBinding::Off);
    }
    let client_id = text(CLIENT_ID).flatten().filter(|id| client_id_in_form(id));
    let redirect_uri = text(REDIRECT_URI)
        .flatten()
        .and_then(|uri| RedirectUri::parse(&uri).ok());
    let address = text(VAULT_ADDRESS)
        .flatten()
        .filter(|address| address_in_form(address));
    let dir = text(VAULT_DIR)
        .flatten()
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute());
    let ca_pem = dir.as_deref().and_then(ca_pem);
    match (client_id, redirect_uri, address, dir, ca_pem) {
        (Some(client_id), Some(redirect_uri), Some(address), Some(dir), Some(ca_pem)) => {
            Ok(ConnectBinding::On(Box::new(BoundConnect {
                authorizer: Authorizer {
                    page,
                    client: OAuthClient {
                        client_id: ClientId(client_id),
                        redirect_uri,
                    },
                },
                login: OpenBaoLogin {
                    config: OpenBaoConfig {
                        address,
                        ca_pem,
                        mounts: Mounts::default(),
                    },
                    dir,
                },
            })))
        }
        (client_id, redirect_uri, address, dir, ca_pem) => {
            let valid = [
                client_id.is_some(),
                redirect_uri.is_some(),
                address.is_some(),
                dir.is_some() && ca_pem.is_some(),
            ];
            Ok(ConnectBinding::Incomplete(
                CONNECT_SETTINGS
                    .into_iter()
                    .zip(valid)
                    .filter(|(_, valid)| !valid)
                    .map(|(name, _)| name)
                    .collect(),
            ))
        }
    }
}

/// Whether `id` is 1 to [`MAX_CLIENT_ID_LEN`] bytes from `!` to `~`.
fn client_id_in_form(id: &str) -> bool {
    (1..=MAX_CLIENT_ID_LEN).contains(&id.len()) && id.bytes().all(|b| (b'!'..=b'~').contains(&b))
}

/// Whether `address` is `https://` and an authority with nothing after it.
fn address_in_form(address: &str) -> bool {
    address.strip_prefix("https://").is_some_and(|authority| {
        !authority.is_empty()
            && authority
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
    })
}

/// The CA certificate in `dir`, read by `mandate-vault`'s mounted-file rules.
fn ca_pem(dir: &Path) -> Option<Vec<u8>> {
    read_mounted(dir, VAULT_CA)
        .ok()
        .map(|pem| pem.expose_secret().as_bytes().to_vec())
}

/// The connect `binding` binds: for [`ConnectBinding::On`], its page and client, states drawn from
/// `random`, and codes written through the vault as the API's identity at `clock`'s time; `None`
/// otherwise.
pub fn bound(
    binding: ConnectBinding,
    random: Box<dyn Random>,
    clock: Arc<dyn Clock>,
) -> Option<Arc<Connect>> {
    match binding {
        ConnectBinding::On(bound) => Some(Arc::new(Connect::new(
            bound.authorizer,
            random,
            Arc::new(VaultCodes {
                login: bound.login,
                clock,
            }),
        ))),
        ConnectBinding::Off | ConnectBinding::Incomplete(_) => None,
    }
}
