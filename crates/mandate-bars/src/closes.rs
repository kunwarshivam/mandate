//! The closes a model output rested on, as the artifact its `ObservationRecorded` names (data-plane
//! spec §3.6, DEC-503 items 5 and 6): `{"closes": [[date, close], ...], "instrument_id": ...}` in
//! canonical JSON, moved unchanged from `mandate-paper` (DEC-2385) so the runner, the runtime
//! process and the data process that stores it build the same bytes, and so the same digest.

use mandate_canon::{Key, Object, Value, to_canonical};
use mandate_num::Price;
use mandate_time::Date;

/// A member name the canonical form refused; none of the two fixed names ever is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the closes object's member names are not canonical keys")]
pub struct MemberRefused;

/// The canonical bytes of `closes`, oldest first as given, for the instrument `asset_id`.
///
/// # Errors
/// [`MemberRefused`] if a member name is not a canonical key.
pub fn closes_object(closes: &[(Date, Price)], asset_id: &str) -> Result<Vec<u8>, MemberRefused> {
    let pair = |(day, close): &(Date, Price)| {
        Value::Array(vec![
            Value::Str(day.to_string()),
            Value::Str(close.to_string()),
        ])
    };
    let mut object = Object::new();
    for (name, value) in [
        ("closes", Value::Array(closes.iter().map(pair).collect())),
        ("instrument_id", Value::Str(asset_id.to_owned())),
    ] {
        let key = Key::new(name).map_err(|_| MemberRefused)?;
        object.insert(key, value);
    }
    Ok(to_canonical(&Value::Object(object)))
}
