//! Family W's `kind: tripwire` reference-case boundary (E6-13).
//!
//! The tests PR names the owned arm but deliberately interprets no case member. Returning an error
//! before any expectation is inspected prevents MC-W27 to MC-W56 from passing until the
//! implementation PR supplies the complete account-stream adapter.

use crate::Json;

/// Refuses every tripwire reference case until E6-13's implementation PR.
pub(super) fn tripwire_case(_case: &Json) -> Result<(), String> {
    Err(
        "the E6-13 tripwire reference adapter is not implemented yet, so this case cannot be said \
         to pass"
            .to_owned(),
    )
}
