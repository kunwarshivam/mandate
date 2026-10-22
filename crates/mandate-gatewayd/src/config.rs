//! What the service is started with, read from its environment (E21-62, DEC-2272 item 7, DEC-2207):
//! every value is required and none has a default, so a value the chart does not render, the
//! spending caps above all, stops the start with its code rather than running on a number no one
//! named (DEC-79). Each is read once, at start.

use mandate_gateway::money::Usd6;

/// The service's start-up values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The one workspace it serves (DEC-2265 item 7), `MANDATE_GATEWAY_WORKSPACE`.
    pub workspace: String,
    /// The deployment whose meter stream it writes (DEC-2207), `MANDATE_GATEWAY_DEPLOYMENT`.
    pub deployment: String,
    /// The workspace journal, `MANDATE_JOURNAL_DSN`. Never printed.
    pub journal_dsn: String,
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

impl ConfigError {
    /// The code the process exits with.
    pub fn code(self) -> &'static str {
        match self {
            Self::WorkspaceMissing => "workspace_missing",
            Self::DeploymentMissing => "deployment_missing",
            Self::JournalMissing => "journal_dsn_missing",
            Self::ArtifactsMissing => "artifact_store_unavailable",
            Self::WorkspaceBudget => "workspace_budget_missing",
            Self::DeploymentBudget => "deployment_budget_missing",
        }
    }
}

impl Config {
    /// The values `var` gives, an empty one read as missing.
    ///
    /// # Errors
    /// The first [`ConfigError`], in the order of [`Config`]'s fields.
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let named = |name: &str, missing: ConfigError| {
            var(name).filter(|value| !value.is_empty()).ok_or(missing)
        };
        let amount = |name: &str, missing: ConfigError| {
            named(name, missing).and_then(|text| Usd6::parse(&text).map_err(|_| missing))
        };
        Ok(Self {
            workspace: named("MANDATE_GATEWAY_WORKSPACE", ConfigError::WorkspaceMissing)?,
            deployment: named("MANDATE_GATEWAY_DEPLOYMENT", ConfigError::DeploymentMissing)?,
            journal_dsn: named("MANDATE_JOURNAL_DSN", ConfigError::JournalMissing)?,
            artifacts: named("MANDATE_ARTIFACTS", ConfigError::ArtifactsMissing)?,
            workspace_monthly: amount(
                "MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD",
                ConfigError::WorkspaceBudget,
            )?,
            deployment_monthly: amount(
                "MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD",
                ConfigError::DeploymentBudget,
            )?,
        })
    }
}
