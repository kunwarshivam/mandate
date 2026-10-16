//! The rehearsal script (DEC-922 item 4, DEC-926).

use mandate_num::{Price, Qty};

use crate::{Account, Side, Sim, SimError};

/// One scripted fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedFill {
    pub symbol: String,
    pub side: Side,
    pub quantity: Option<Qty>,
    pub price: Price,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptError {
    #[error("the script is not a JSON object")]
    NotJson,
    #[error(transparent)]
    Sim(#[from] SimError),
}

/// A parsed script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    accounts: Vec<Account>,
    fills: Vec<ScriptedFill>,
}

impl Script {
    pub fn parse(_text: &str) -> Result<Self, ScriptError> {
        Err(ScriptError::NotJson)
    }

    pub fn sim(&self) -> Result<Sim, SimError> {
        Sim::new(self.accounts.clone())
    }

    pub fn fills(&self) -> &[ScriptedFill] {
        &self.fills
    }
}
