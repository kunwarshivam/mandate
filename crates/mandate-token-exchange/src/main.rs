//! `mandate-token-exchange run --vault-address <address> --vault-dir <dir>` (connections spec §5.2
//! step 4, DEC-1175, DEC-2629, DEC-2330): the one process that sends an OAuth connect's live token
//! request. It reads its settings from the environment the deployment manager gives its pod, then
//! exactly those arguments, logs in to the vault as its connection's exchange, and exchanges.
//!
//! Every outcome but success writes one code alone to standard error:
//! - exit 2: a start refusal, the settings', the arguments' or the request's deadline's
//!   (DEC-2363 item 2), before the vault or the network;
//! - exit 4: the vault could not be logged in to;
//! - exit 5: the exchange refused or failed, with the connect core's code.
//!
//! Exit 0 writes nothing: the token is in the vault.

use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

use mandate_token_exchange::process::{ARGUMENTS_REFUSED, Failed, exchange, now, vault_args};
use mandate_token_exchange::startup::{REQUEST_EXPIRES_AT, request_deadline, settings};
use mandate_vault::mounted::SERVICE_ACCOUNT_TOKEN_DIR;

/// The exit code of every start refusal.
const REFUSED: u8 = 2;

/// The exit code when the vault cannot be logged in to.
const VAULT_FAILED: u8 = 4;

/// The exit code when the exchange refuses or fails.
const EXCHANGE_FAILED: u8 = 5;

/// Writes `code` alone and exits `status`.
fn ended(code: &str, status: u8) -> ExitCode {
    let _ = writeln!(std::io::stderr().lock(), "{code}");
    ExitCode::from(status)
}

fn main() -> ExitCode {
    let settings = match settings(|name| std::env::var_os(name)) {
        Ok(settings) => settings,
        Err(refused) => return ended(refused.code(), REFUSED),
    };
    let Some(args) = vault_args(std::env::args_os().skip(1)) else {
        return ended(ARGUMENTS_REFUSED, REFUSED);
    };
    let Some(now) = now() else {
        return ended(Failed::Vault.code(), VAULT_FAILED);
    };
    let deadline = match request_deadline(std::env::var_os(REQUEST_EXPIRES_AT).as_deref(), now) {
        Ok(deadline) => deadline,
        Err(refused) => return ended(refused.code(), REFUSED),
    };
    let token_dir = Path::new(SERVICE_ACCOUNT_TOKEN_DIR);
    match exchange(&settings, &args, token_dir, now, deadline) {
        Ok(_) => ExitCode::SUCCESS,
        Err(failed @ Failed::Vault) => ended(failed.code(), VAULT_FAILED),
        Err(failed @ Failed::Exchange(_)) => ended(failed.code(), EXCHANGE_FAILED),
    }
}
