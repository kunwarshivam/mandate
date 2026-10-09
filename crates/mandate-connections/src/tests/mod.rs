//! Tests of the connect core, grouped by the module they test, with `support`'s fixture vault and
//! token endpoint and no network. A test marked `#[ignore = "pending <story>"]` fails at that
//! story's `Unimplemented` stub until the story's code lands; every other test runs.

mod checks;
mod errors;
mod exchange;
mod grant;
mod hosts;
mod manager;
mod record;
mod revoke;
mod start;
mod support;
