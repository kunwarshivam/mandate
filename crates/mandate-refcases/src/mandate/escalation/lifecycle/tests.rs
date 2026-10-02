//! The map's own pieces, on inputs no reference case reaches: a name bound twice, a name or a
//! runtime id missing, and a reference mark the runtime writes with a member too many.

use std::collections::BTreeMap;

use mandate_canon::Value;
use serde_json::json;

use super::{As, bind, member, object, seconds, text};
use crate::ensure;

/// A name binds to one runtime name and a runtime name to one name, and each must be present.
#[test]
fn names_bind_one_to_one() -> Result<(), String> {
    let mut bound = BTreeMap::new();
    bind(&mut bound, Some("ap1"), Some("R1"))?;
    bind(&mut bound, Some("ap1"), Some("R1"))?;
    bind(&mut bound, Some("ap2"), Some("R2"))?;
    for (name, runtime) in [
        (Some("ap1"), Some("R3")),
        (Some("ap3"), Some("R1")),
        (None, Some("R4")),
        (Some("ap4"), None),
    ] {
        ensure(bind(&mut bound, name, runtime).is_err(), || {
            format!("{name:?} bound to {runtime:?}")
        })?;
    }
    ensure(bound.len() == 2, || format!("{bound:?}"))
}

/// A reference mark is exactly `{price, seq}`: a member the map does not name fails it.
#[test]
fn a_reference_mark_has_exactly_its_members() -> Result<(), String> {
    let want = json!({"price": "155", "seq": 2});
    let mark = |extra: bool| -> Result<Value, String> {
        let mut members = vec![("price", text("155")), ("seq", seconds(2)?)];
        if extra {
            members.push(("feed", text("sip")));
        }
        object(members)
    };
    member(As::Mark, &want, Some(&mark(false)?))?;
    ensure(member(As::Mark, &want, Some(&mark(true)?)).is_err(), || {
        "an extra member passed".to_owned()
    })
}
