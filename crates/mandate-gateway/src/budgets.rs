//! Model-spend budgets as settings ([DEC-2204](../../../docs/project/decisions/DEC-2204.md) item 1,
//! DEC-2203 item 9): the deployment's monthly cap and each workspace's monthly budget come from
//! deployment configuration, never from a figure in code. A setting that is missing or unreadable
//! is an error, and a gateway without its budgets makes no call (`AGENTS.md` rule 3).

use crate::money::Usd6;

/// The deployment's monthly cap on model spend, in USD (DEC-997 item 3's figure, Proposed).
pub const DEPLOYMENT_MONTHLY_VAR: &str = "MANDATE_MODEL_DEPLOYMENT_MONTHLY_USD";
/// The monthly model budget of each workspace, until a workspace policy key sets its own.
pub const WORKSPACE_MONTHLY_VAR: &str = "MANDATE_MODEL_WORKSPACE_MONTHLY_USD";

/// The two monthly budgets a gateway is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    pub deployment_monthly: Usd6,
    pub workspace_monthly: Usd6,
}

/// Why the budgets could not be read; the variable is named, its value never is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BudgetError {
    #[error("{0} is not set")]
    Missing(&'static str),
    #[error("{0} is not a non-negative USD amount with at most six decimals")]
    Invalid(&'static str),
}

impl Budgets {
    /// Reads both budgets through `lookup`, the process environment in a cell.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, BudgetError> {
        let read = |var: &'static str| -> Result<Usd6, BudgetError> {
            let text = lookup(var).ok_or(BudgetError::Missing(var))?;
            Usd6::parse(text.trim()).map_err(|_| BudgetError::Invalid(var))
        };
        Ok(Self {
            deployment_monthly: read(DEPLOYMENT_MONTHLY_VAR)?,
            workspace_monthly: read(WORKSPACE_MONTHLY_VAR)?,
        })
    }
}
