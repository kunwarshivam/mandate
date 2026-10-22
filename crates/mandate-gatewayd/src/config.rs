//! What the service is started with, read from its environment (E21-62, DEC-2272 item 7, DEC-2207):
//! every value is required and none has a default, so a value the chart does not render, the
//! spending caps above all, stops the start with its code rather than running on a number no one
//! named (DEC-79). Each is read once, at start.

use mandate_gateway::money::Usd6;
use secrecy::SecretString;

/// The service's start-up values. Its `Debug` shows the journal's DSN only as redacted, since a
/// DSN carries a password (`AGENTS.md` rule 7).
#[derive(Debug)]
pub struct Config {
    /// The one workspace it serves (DEC-2265 item 7), `MANDATE_GATEWAY_WORKSPACE`.
    pub workspace: String,
    /// The deployment whose meter stream it writes (DEC-2207), `MANDATE_GATEWAY_DEPLOYMENT`.
    pub deployment: String,
    /// The workspace journal, `MANDATE_JOURNAL_DSN`, held so that nothing prints it.
    journal_dsn: SecretString,
    /// The artifact store, mounted read-only (DEC-2272 item 7), `MANDATE_ARTIFACTS`.
    pub artifacts: String,
    /// The workspace's monthly model budget, `MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD`.
    pub workspace_monthly: Usd6,
    /// The deployment's monthly model budget, `MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD`.
    pub deployment_monthly: Usd6,
}

/// Why the service did not start: the first value missing or malformed, by its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("no workspace is named")]
    WorkspaceMissing,
    #[error("no deployment is named")]
    DeploymentMissing,
    #[error("no journal is named")]
    JournalMissing,
    #[error("no artifact store is mounted")]
    ArtifactsMissing,
    #[error("the workspace's monthly budget is missing or not an amount")]
    WorkspaceBudget,
    #[error("the deployment's monthly budget is missing or not an amount")]
    DeploymentBudget,
}

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
impl ConfigError {
    /// The code the process exits with.
    pub fn code(self) -> &'static str {
        let _ = self;
        todo!()
    }
}

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
impl Config {
    /// The journal's DSN, for opening the journal and nothing else.
    pub fn journal_dsn(&self) -> &str {
        let _ = &self.journal_dsn;
        todo!()
    }

    /// The values `var` gives, an empty one read as missing.
    ///
    /// # Errors
    /// The first [`ConfigError`], in the order of [`Config`]'s fields.
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let _ = var;
        todo!()
    }
}
