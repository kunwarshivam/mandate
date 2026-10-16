//! The rehearsal script (DEC-922 item 4, [DEC-926]): what the founder's rehearsal serves, read
//! from one JSON object. `accounts` is required; `session` (`closed`, `overnight`, `extended` or
//! `regular`, closed when absent), `quotes` (symbol to price) and `fills` are not. A fill names a
//! symbol, a side, a price and, optionally, a quantity, the whole unfilled quantity when absent;
//! the server uses it once, on the first placed order of that symbol and side it can fill.
//!
//! Read strictly: a member the script does not define, a number written as a JSON number rather
//! than canonical decimal text, or a value outside its enumeration refuses the whole script, so
//! a typo never becomes a silent default.
//!
//! [DEC-926]: ../../../docs/project/decisions/DEC-926.md

use mandate_num::{Price, Qty, Usd};
use serde_json::{Map, Value};

use crate::{Account, Event, Session, Side, Sim, SimError};

/// One scripted fill: the first placed order of `symbol` and `side` that the core lets fill
/// takes `quantity`, or its whole unfilled quantity, at `price`.
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
    #[error("`{0}` is missing, unreadable, or not a member the script defines")]
    Member(&'static str),
    #[error(transparent)]
    Sim(#[from] SimError),
}

/// A script read in full: the accounts, the session, the quotes and the fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    accounts: Vec<Account>,
    session: Session,
    quotes: Vec<(String, Price)>,
    fills: Vec<ScriptedFill>,
}

impl Script {
    /// The script `text` holds, or the first thing in it that is not one.
    pub fn parse(text: &str) -> Result<Self, ScriptError> {
        let value: Value = serde_json::from_str(text).map_err(|_| ScriptError::NotJson)?;
        let top = value.as_object().ok_or(ScriptError::NotJson)?;
        only(top, &["accounts", "session", "quotes", "fills"], "script")?;
        let accounts = list(top.get("accounts"), "accounts")?
            .iter()
            .map(account)
            .collect::<Result<Vec<_>, _>>()?;
        let session = match top.get("session").map(Value::as_str) {
            None => Session::Closed,
            Some(Some("closed")) => Session::Closed,
            Some(Some("overnight")) => Session::Overnight,
            Some(Some("extended")) => Session::Extended,
            Some(Some("regular")) => Session::Regular,
            Some(_) => return Err(ScriptError::Member("session")),
        };
        let quotes = match top.get("quotes") {
            None => Vec::new(),
            Some(Value::Object(quotes)) => quotes
                .iter()
                .map(|(symbol, quote)| {
                    let quote = quote.as_str().ok_or(ScriptError::Member("quotes"))?;
                    let quote = Price::parse(quote).map_err(|_| ScriptError::Member("quotes"))?;
                    Ok((symbol.clone(), quote))
                })
                .collect::<Result<Vec<_>, ScriptError>>()?,
            Some(_) => return Err(ScriptError::Member("quotes")),
        };
        let fills = match top.get("fills") {
            None => Vec::new(),
            some => list(some, "fills")?
                .iter()
                .map(fill)
                .collect::<Result<Vec<_>, _>>()?,
        };
        let script = Self {
            accounts,
            session,
            quotes,
            fills,
        };
        script.sim()?;
        Ok(script)
    }

    /// A fresh core over the script's accounts, with its quotes and in its session.
    pub fn sim(&self) -> Result<Sim, SimError> {
        let mut sim = Sim::new(self.accounts.clone())?;
        for (symbol, quote) in &self.quotes {
            sim.apply(Event::Quote(symbol.clone(), *quote))?;
        }
        sim.apply(Event::Session(self.session))?;
        Ok(sim)
    }

    /// The scripted fills, in the order the script lists them.
    pub fn fills(&self) -> &[ScriptedFill] {
        &self.fills
    }
}

/// Refuses an object with a member `allowed` does not name.
fn only(
    object: &Map<String, Value>,
    allowed: &[&str],
    what: &'static str,
) -> Result<(), ScriptError> {
    if object.keys().all(|key| allowed.contains(&key.as_str())) {
        Ok(())
    } else {
        Err(ScriptError::Member(what))
    }
}

fn list<'a>(value: Option<&'a Value>, what: &'static str) -> Result<&'a [Value], ScriptError> {
    value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or(ScriptError::Member(what))
}

fn object<'a>(value: &'a Value, what: &'static str) -> Result<&'a Map<String, Value>, ScriptError> {
    value.as_object().ok_or(ScriptError::Member(what))
}

fn text<'a>(object: &'a Map<String, Value>, key: &'static str) -> Result<&'a str, ScriptError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ScriptError::Member(key))
}

fn flag(object: &Map<String, Value>, key: &'static str) -> Result<bool, ScriptError> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or(ScriptError::Member(key))
}

fn account(value: &Value) -> Result<Account, ScriptError> {
    let fields = object(value, "accounts")?;
    let names = [
        "number",
        "agentic_allowed",
        "cash",
        "buying_power",
        "pattern_day_trader",
    ];
    only(fields, &names, "accounts")?;
    let usd =
        |key: &'static str| Usd::parse(text(fields, key)?).map_err(|_| ScriptError::Member(key));
    Ok(Account {
        number: text(fields, "number")?.to_owned(),
        agentic_allowed: flag(fields, "agentic_allowed")?,
        cash: usd("cash")?,
        buying_power: usd("buying_power")?,
        pattern_day_trader: flag(fields, "pattern_day_trader")?,
    })
}

fn fill(value: &Value) -> Result<ScriptedFill, ScriptError> {
    let fields = object(value, "fills")?;
    only(fields, &["symbol", "side", "price", "quantity"], "fills")?;
    let side = match text(fields, "side")? {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => return Err(ScriptError::Member("side")),
    };
    let price = Price::parse(text(fields, "price")?).map_err(|_| ScriptError::Member("price"))?;
    let quantity = match fields.get("quantity") {
        None => None,
        Some(_) => {
            let quantity = Qty::parse(text(fields, "quantity")?)
                .map_err(|_| ScriptError::Member("quantity"))?;
            if quantity.is_zero() {
                return Err(ScriptError::Member("quantity"));
            }
            Some(quantity)
        }
    };
    Ok(ScriptedFill {
        symbol: text(fields, "symbol")?.to_owned(),
        side,
        quantity,
        price,
    })
}
